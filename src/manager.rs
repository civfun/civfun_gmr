use crate::api::{Api, DownloadMessage, Game, GameId, GetGamesAndPlayers, UserId};
use crate::{data_dir_path, project_dirs};
use anyhow::anyhow;
use anyhow::Context;
use civ5save::{Civ5Save, Civ5SaveReader};
use directories::BaseDirs;
use notify::{DebouncedEvent, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, RwLockWriteGuard};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, trace, trace_span, warn};

type Result<T> = anyhow::Result<T>;

const CONFIG_KEY: &str = "config";
const GAME_API_RESPONSE_KEY: &str = "data";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AuthState {
    Nothing,
    Fetching,
    AuthResult(Option<UserId>),
}

impl Default for AuthState {
    fn default() -> Self {
        AuthState::Nothing
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    auth_key: Option<String>,
    auth_state: AuthState,
}

#[derive(Debug, Clone)]
pub struct GameInfo {
    pub game: Game,
    pub download: Option<Download>,
    pub parsed: Option<Civ5Save>,
}

#[derive(Debug, Default)]
struct Inner {
    config: Config,
    games: GetGamesAndPlayers,
    download_state: HashMap<GameId, Download>,
    download_rx: HashMap<GameId, Receiver<DownloadMessage>>,
    new_files_seen: Vec<String>,
    parsed_saves: HashMap<GameId, Civ5Save>,
}

impl Inner {
    fn games(&self) -> Vec<GameInfo> {
        self.games
            .games
            .iter()
            .map(|game| GameInfo {
                game: game.clone(),
                download: self.download_state.get(&game.game_id).cloned(),
                parsed: self.parsed_saves.get(&game.game_id).cloned(),
            })
            .collect()
    }

    fn my_games(&self) -> Result<Vec<GameInfo>> {
        let user_id = match &self.config.auth_state {
            AuthState::AuthResult(Some(user_id)) => user_id,
            _ => return Err(anyhow!("my_games requested without a valid auth state.")),
        };

        Ok(self
            .games()
            .into_iter()
            .filter(|g| g.game.is_user_id_turn(user_id))
            .collect())
    }
}

#[derive(Debug, Clone)]
pub enum Download {
    Idle,
    Downloading,
    Complete,
}

#[derive(Debug, Clone)]
pub struct Manager {
    db: sled::Db,
    inner: Arc<RwLock<Inner>>,
}

impl Manager {
    pub fn new() -> Result<Self> {
        let db_path =
            data_dir_path(&PathBuf::from("db.sled")).context("Constructing db.sled path")?;
        debug!("DB path: {:?}", &db_path);
        let db = sled::open(&db_path)
            .with_context(|| format!("Could not create db at {:?}", &db_path))?;

        let mut s = Self {
            db,
            inner: Default::default(),
        };
        s.load_config()?;
        s.load_games()?;

        Ok(s)
    }

    #[instrument(skip(self))]
    pub async fn authenticate(&mut self) -> Result<Option<UserId>> {
        let maybe_user_id = self.api()?.authenticate_user().await?;
        debug!("User ID response: {:?}", maybe_user_id);
        let mut inner = self.inner.write().unwrap();
        inner.config.auth_state = AuthState::AuthResult(maybe_user_id);
        self.save_config(&mut inner)?;
        Ok(maybe_user_id)
    }

    /// Ready means we have an auth key and a user id.
    pub fn all_ready(&self) -> bool {
        self.auth_ready() && self.user_ready()
    }

    pub fn auth_ready(&self) -> bool {
        self.inner.read().unwrap().config.auth_key.is_some()
    }

    pub fn user_ready(&self) -> bool {
        match self.inner.read().unwrap().config.auth_state {
            AuthState::AuthResult(Some(_)) => true,
            _ => false,
        }
    }

    /// This will eventually fetch a second time if the players shown don't exist in the db.
    /// It will also start downloading games if they don't exist.
    pub async fn refresh(&mut self) -> Result<()> {
        let games = self.api()?.get_games_and_players(&[]).await?;
        dbg!("???? 1");
        self.save_games(games)?;
        // TODO
        // let unknown_players = self.filter_unknown_players();
        // if unknown_players.len() > 0 {
        //     let data = self.api()?.get_games_and_players([]).await?;
        // }
        dbg!("???? 2");
        self.start_downloads().await.unwrap();
        dbg!("???? 3");
        Ok(())
    }

    pub fn user_id(&self) -> Option<UserId> {
        if let AuthState::AuthResult(maybe_id) = self.inner.read().unwrap().config.auth_state {
            maybe_id
        } else {
            None
        }
    }

    #[instrument(skip(self))]
    pub async fn start_downloads(&mut self) -> Result<Option<()>> {
        // If we don't have a user_id, don't bother trying.
        let user_id = match self.user_id() {
            None => return Ok(None),
            Some(u) => u,
        };
        let games = {
            let inner = self.inner.read().unwrap();
            inner.games.clone()
        };

        // TODO: Use inner.my_games()
        for game in &games.games {
            if !game.is_user_id_turn(&user_id) {
                continue;
            }

            {
                let mut inner = self.inner.read().unwrap();
                match inner.download_state.get(&game.game_id) {
                    None => {}
                    Some(Download::Idle) => {}
                    _ => continue,
                }
            }

            let game_id = game.game_id.clone();
            let path = Self::save_dir()?.join(Self::filename(&game)?);
            let (rx, handle) = self
                .api()?
                .get_latest_save_file_bytes(&game_id, &path)
                .await?;

            {
                let mut inner = self.inner.write().unwrap();
                inner.download_state.insert(game_id, Download::Downloading);
                inner.download_rx.insert(game_id, rx);
            }
        }

        Ok(Some(())) // TODO: return something useful?
    }

    fn saved_bytes_db_key(game_id: &GameId) -> String {
        format!("saved-bytes-{}", game_id)
    }

    /// Windows: ~\Documents\My Games\Sid Meier's Civilization 5\Saves\hotseat\
    /// OS X: ~/Documents/Aspyr/Sid Meier's Civilization 5/Saves/hotseat/
    /// Linux: ~/.local/share/Aspyr/Sid Meier's Civilization 5/Saves/hotseat/
    fn save_dir() -> Result<PathBuf> {
        let base_dirs = BaseDirs::new().ok_or(anyhow!("Could not work out basedir."))?;
        let home = base_dirs.home_dir();
        let suffix = PathBuf::from("Sid Meier's Civilization 5")
            .join("Saves")
            .join("hotseat");
        // Can't use the `directories` crate because these paths are inconsistent between OS's.
        let middle = if cfg!(windows) {
            PathBuf::from("Documents").join("My Games")
        } else if cfg!(target_os = "macos") {
            PathBuf::from("Documents").join("Aspyr")
        } else if cfg!(unix) {
            PathBuf::from(".local").join("share").join("Aspyr")
        } else {
            return Err(anyhow!("Unhandled operating system for save_dir."));
        };
        Ok(home.join(middle).join(suffix))
    }

    fn filename(game: &Game) -> Result<PathBuf> {
        let cleaner_name: String = game
            .name
            .chars()
            .map(|c| match "./\\\"<>|:*?".contains(c) {
                true => '_',
                false => c,
            })
            .collect();
        Ok(format!("(civfun {}) {}.Civ5Save", game.game_id, cleaner_name).into())
    }

    pub fn process_downloads(&self) {
        let games = {
            let inner = self.inner.read().unwrap();
            inner.games.clone()
        };

        for game in &games.games {
            let span = trace_span!("", id = ?game.game_id);
            let _enter = span.enter();

            let mut inner = self.inner.write().unwrap();
            let mut update_state = None;
            if let Some(Download::Downloading) = inner.download_state.get_mut(&game.game_id) {
                // TODO: This is probably a bug if it panics.
                let rx = inner.download_rx.get_mut(&game.game_id).unwrap();
                loop {
                    let msg = match rx.try_recv() {
                        Ok(msg) => msg,
                        Err(TryRecvError::Empty) => break,
                        Err(err) => panic!("{:?}", err),
                    };
                    match msg {
                        DownloadMessage::Error(e) => {
                            error!(?e, "Download");
                        }
                        DownloadMessage::Started(size) => {
                            trace!(?size, "Started");
                        }
                        DownloadMessage::Chunk(percentage) => {
                            trace!(?percentage, "Download progress");
                        }
                        DownloadMessage::Done(path) => {
                            trace!("Done!");

                            // Save the file into the DB because:
                            // 1) The user might delete the file in the future
                            // 2) Be able to analyse the file and compare when the user uploads their turn.
                            trace!(?path, "Placing save file into db.");
                            let mut fp = File::open(&path).unwrap();
                            let mut data = Vec::with_capacity(1_000_000);
                            fp.read_to_end(&mut data).unwrap();
                            self.db
                                .insert(Self::saved_bytes_db_key(&game.game_id), data.clone())
                                .unwrap();

                            // Analyse the save
                            trace!(data_len = ?data.len(), "Analysing save.");
                            let civ5save = Civ5SaveReader::new(&data).parse().unwrap();
                            trace!(?civ5save);
                            inner.parsed_saves.insert(game.game_id.clone(), civ5save);

                            update_state = Some(Download::Complete);
                            inner.download_rx.remove(&game.game_id);
                            break;
                        }
                    }
                }
            }
            if let Some(new_state) = update_state {
                inner.download_state.insert(game.game_id, new_state);
            }
        }
    }

    pub fn download_status(&self) -> Vec<Download> {
        todo!()
    }

    #[instrument(skip(self))]
    pub async fn start_watching_saves(&self) -> Result<()> {
        let save_dir = Self::save_dir().unwrap();
        debug!(?save_dir);

        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(250))?;
        watcher.watch(save_dir, RecursiveMode::NonRecursive)?;

        let s = self.clone();
        tokio::spawn(async move {
            // Move watcher into here, since it would be dropped otherwise and then the channel
            // would be dropped.
            let _ = watcher;

            trace!("Loop started.");
            loop {
                match rx.try_recv() {
                    Ok(event) => {
                        info!(?event);
                        if let DebouncedEvent::Create(path) = event {
                            let mut inner = s.inner.write().unwrap();
                            let filename = path.file_name().unwrap().to_str().unwrap().into();
                            inner.new_files_seen.push(filename);
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        warn!("Disconnected");
                        return;
                    }
                }

                tokio::task::yield_now().await;
            }
        });

        Ok(())
    }

    pub fn process_new_saves(&self) -> Result<()> {
        // Atomically get the files out of the inner struct.
        let files = {
            let mut inner = self.inner.write().unwrap();
            let new_files = inner.new_files_seen.clone();
            inner.new_files_seen = vec![];
            new_files
        };

        for file in files {
            self.check_save(&file).unwrap(); // TODO: unwrap
        }

        Ok(())
    }

    ///
    /// Casimir III_0028 BC-2320.Civ5Save
    /// [Next turn's leader]_[Turn number] [(BC|AD)-Year].Civ5Save
    /// Filter current games:
    ///  - When turn number is the same or +1.
    ///  - Verify the difference between the downloaded save and the new file is low.
    /// If there's more than one, there's something really wrong, so abort for now.
    /// Otherwise:
    ///  - Move the originally downloaded file to `civfun Archive/[game_id]_[turn]_[dn]_[original name]`.
    ///  - Copy the file bytes into the DB and queue for upload.
    ///  - Move the uploaded file to `civfun Archive/[game_id]_[turn]_[up]_[original name]`
    #[instrument(skip(self))]
    fn check_save(&self, filename: &str) -> Result<()> {
        let turn = Self::turn_from_filename(filename)?;
        let inner = self.inner.write().unwrap();
        for game in inner.my_games()? {}

        Ok(())
    }

    fn turn_from_filename(filename: &str) -> Result<u32> {
        // TODO: once_cell
        let re = Regex::new(r"(?P<leader>.*?)_(?P<turn>\d{4}) (?P<year>.*?)\.Civ5Save").unwrap();
        let captures = re.captures(&filename).unwrap(); // TODO: unwrap
        trace!(?captures);
        let turn = captures.name("turn").unwrap().as_str();
        let turn: u32 = turn.parse().unwrap();
        Ok(turn)
    }

    pub fn load_config(&mut self) -> Result<()> {
        let config = match self.db.get(CONFIG_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => Config::default(),
        };
        self.inner.write().unwrap().config = config;
        Ok(())
    }

    // RwLockWriteGuard is used here so that a config field can be modified within the same
    // write lock as the caller.
    fn save_config(&self, inner: &mut RwLockWriteGuard<Inner>) -> Result<()> {
        let encoded = serde_json::to_vec(&inner.config)?;
        self.db.insert(CONFIG_KEY, encoded.as_slice())?;
        Ok(())
    }

    pub fn save_auth_key(&mut self, key: &str) -> Result<()> {
        let mut inner = self.inner.write().unwrap();
        inner.config.auth_key = Some(key.to_owned());
        self.save_config(&mut inner)?;
        Ok(())
    }

    pub fn load_games(&mut self) -> Result<()> {
        let data = match self.db.get(GAME_API_RESPONSE_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => GetGamesAndPlayers::default(),
        };

        self.inner.write().unwrap().games = data;
        Ok(())
    }

    pub fn save_games(&self, data: GetGamesAndPlayers) -> Result<()> {
        let mut inner = self.inner.write().unwrap();
        let encoded = serde_json::to_vec(&data)?;
        self.db.insert(GAME_API_RESPONSE_KEY, encoded.as_slice())?;
        inner.games = data;
        Ok(())
    }

    pub fn clear_games(&self) -> Result<()> {
        self.db.remove(GAME_API_RESPONSE_KEY)?;
        Ok(())
    }

    fn api(&self) -> Result<Api> {
        let inner = self.inner.read().unwrap();
        match &inner.config.auth_key {
            Some(auth_key) => Ok(Api::new(auth_key)),
            None => Err(anyhow!("Attempt to access API without auth key.")),
        }
    }

    pub fn games(&self) -> Vec<GameInfo> {
        let inner = self.inner.read().unwrap();
        let games = inner.games();
        games.clone()
    }
}
