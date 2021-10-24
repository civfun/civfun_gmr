use anyhow::anyhow;
use anyhow::Context;
use civ5save::{Civ5Save, Civ5SaveReader};
use directories::{BaseDirs, ProjectDirs};
use notify::{DebouncedEvent, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, RwLockWriteGuard};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::Receiver;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, trace, trace_span, warn};

use crate::api::{
    Api, DownloadMessage, Game, GameId, GetGamesAndPlayers, Player, TurnId, UploadMessage, UserId,
};

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
    expected_user_id: Option<UserId>,

    #[serde(default)]
    auth_state: AuthState,
}

#[derive(Debug, Clone)]
pub struct GameInfo {
    pub game: Game,
    pub download: Option<State>,
    pub parsed: Option<Civ5Save>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPlayer {
    player: Player,
    image_data: Vec<u8>,
    last_downloaded: SystemTime,
}

#[derive(Debug, Default)]
struct Inner {}

#[derive(Debug, Clone)]
pub enum State {
    Idle,
    Downloading,
    Downloaded,
    UploadQueued,
    Uploading,
    UploadComplete,
}

#[derive(Debug)]
pub enum Event {
    AuthenticationSuccess,
    AuthenticationFailure,
    UpdatedGames(Vec<GameInfo>),
    // UpdatedPlayers(Vec<StoredPlayer>),
}

#[derive(Debug)]
pub struct Manager {
    db: sled::Db,

    config: Config,
    games: GetGamesAndPlayers,
    state: HashMap<GameId, State>,
    new_files_seen: Vec<String>,
    parsed_saves: HashMap<GameId, Civ5Save>,

    auth_rx: Option<oneshot::Receiver<Option<UserId>>>,
    download_rx: HashMap<GameId, Receiver<DownloadMessage>>,
    upload_rx: HashMap<GameId, Receiver<UploadMessage>>,
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
            config: Default::default(),
            games: Default::default(),
            state: Default::default(),
            new_files_seen: vec![],
            parsed_saves: Default::default(),
            auth_rx: None,
            download_rx: Default::default(),
            upload_rx: Default::default(),
        };
        s.load_config().context("Loading config.")?;
        s.load_games().context("Loading existing games.")?;

        Ok(s)
    }

    pub fn process(&mut self) -> Result<Option<Event>> {
        if let Some(ref mut rx) = self.auth_rx {
            match rx.try_recv() {
                Ok(maybe_user_id) => {
                    if let Some(event) = self
                        .handle_auth_response(maybe_user_id)
                        .with_context(|| format!("Handling auth response: {:?}", &maybe_user_id))?
                    {
                        return Ok(Some(event));
                    }
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {}
            };
        }

        Ok(None)
    }

    pub fn games(&self) -> Vec<GameInfo> {
        self.games
            .games
            .iter()
            .map(|game| GameInfo {
                game: game.clone(),
                download: self.state.get(&game.game_id).cloned(),
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

    #[instrument(skip(self, key))]
    pub fn authenticate(&mut self, key: &str) {
        trace!("Authentication requested.");
        let (tx, rx) = oneshot::channel();
        self.auth_rx = Some(rx);
        self.config.auth_key = Some(key.to_string());
        // This shouldn't panic because we just gave the API key.
        // TODO: Maybe be more explicit when getting an api instance, instead of calling self.api()?
        let api = self.api().unwrap();

        tokio::spawn(async move {
            trace!("Sending authentication request.");
            let maybe_user_id = api.authenticate_user().await.unwrap();
            debug!(?maybe_user_id, "User ID response.");
            tx.send(maybe_user_id).unwrap();
        });
    }

    #[instrument(skip(self))]
    fn handle_auth_response(&mut self, maybe_user_id: Option<UserId>) -> Result<Option<Event>> {
        trace!("Handling auth response.");

        self.config.auth_state = AuthState::AuthResult(maybe_user_id);

        // The user_id has changed so we reset the games.
        if let Some(user_id) = maybe_user_id {
            if let Some(expected_user_id) = self.config.expected_user_id {
                if expected_user_id != user_id {
                    info!("Clearing games because user_id is different");
                    self.clear_games().context("Clear games 1.")?
                } else {
                    debug!("Same user as last login.")
                }
            } else {
                info!("Clearing games because of no previous user_id.");
                self.clear_games().context("Clear games 2.")?
            }
            self.config.expected_user_id = Some(user_id);
            self.save_config()?;
            Ok(Some(Event::AuthenticationSuccess))
        } else {
            self.save_config()?;
            warn!("Failed to authenticate.");
            Ok(Some(Event::AuthenticationFailure))
        }
    }

    /// Ready means we have an auth key and a user id.
    pub fn all_ready(&self) -> bool {
        self.auth_ready() && self.user_ready()
    }

    pub fn auth_ready(&self) -> bool {
        self.config.auth_key.is_some()
    }

    pub fn user_ready(&self) -> bool {
        match self.config.auth_state {
            AuthState::AuthResult(Some(_)) => true,
            _ => false,
        }
    }

    /// This will eventually fetch a second time if the players shown don't exist in the db.
    /// It will also start downloading games if they don't exist.
    #[instrument(skip(self))]
    pub async fn refresh(&mut self) -> Result<()> {
        let games = self.api()?.get_games_and_players(&[]).await?;
        self.save_games(&games)?;
        let unknown_players = self
            .filter_unknown_players(&games)
            .context("Filter unknown players.")?;
        if unknown_players.len() > 0 {
            let data = self
                .api()?
                .get_games_and_players(unknown_players.as_slice())
                .await?;

            for player in data.players {
                debug!(avatar_url = ?player.avatar_url, "Fetching avatar.");
                let db = self.db.clone();
                let player = player.clone();
                tokio::spawn(async move {
                    Self::fetch_avatar(player, db).await;
                });
            }
        }
        Ok(())
    }

    #[instrument(skip(db))]
    async fn fetch_avatar(player: Player, db: sled::Db) {
        let image_data = reqwest::get(&player.avatar_url)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .to_vec();
        let key = Self::player_info_key(&player.steam_id);

        let stored_player = StoredPlayer {
            player,
            image_data,
            last_downloaded: SystemTime::now(),
        };

        let json = serde_json::to_vec(&stored_player).unwrap();
        db.insert(key, json).unwrap();
    }

    fn filter_unknown_players(&self, games: &GetGamesAndPlayers) -> Result<Vec<UserId>> {
        let mut players: Vec<UserId> = games
            .games
            .iter()
            .map(|g| g.players.iter().map(|p| p.user_id))
            .flatten()
            .collect();
        players.sort();
        players.dedup();

        let mut needs_request = vec![];
        for user_id in players {
            let key = Self::player_info_key(&user_id);
            let data = self
                .db
                .get(&key)
                .with_context(|| format!("Player info key: {}", &key))?;

            match data {
                Some(u) => {
                    // TODO: Check the age of the avatar, e.g. 24 hours and add to needs_request.
                }
                None => {
                    needs_request.push(user_id);
                }
            }
        }
        Ok(needs_request)
    }

    pub fn user_id(&self) -> Option<UserId> {
        if let AuthState::AuthResult(maybe_id) = self.config.auth_state {
            maybe_id
        } else {
            None
        }
    }

    fn player_info_key(user_id: &UserId) -> String {
        format!("player-info-{}", user_id)
    }

    fn saved_bytes_db_key(game_id: &GameId, turn_id: &TurnId) -> String {
        format!("saved-bytes-{}-{}", game_id, turn_id)
    }

    fn upload_bytes_db_key(game_id: &GameId) -> String {
        format!("upload-bytes-{}", game_id)
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

    #[instrument(skip(self))]
    fn store_downloaded_save(
        &mut self,
        game_id: &GameId,
        turn_id: &TurnId,
        path: &PathBuf,
    ) -> anyhow::Result<()> {
        trace!("Placing save file into db.");
        let mut fp = File::open(&path)?;
        let mut data = Vec::with_capacity(1_000_000);
        fp.read_to_end(&mut data)?;
        self.db.insert(
            Self::saved_bytes_db_key(&game_id, &turn_id),
            data.as_slice(),
        )?;
        self.state.insert(game_id.clone(), State::Downloaded);

        self.analyse_download(game_id, &data)?;

        Ok(())
    }

    fn analyse_download(&mut self, game_id: &GameId, data: &[u8]) -> Result<()> {
        trace!(data_len = ?data.len(), "Analysing save.");
        let civ5save = Civ5SaveReader::new(&data).parse()?;
        trace!(?civ5save);
        self.parsed_saves.insert(game_id.clone(), civ5save);
        Ok(())
    }

    pub fn download_status(&self) -> Vec<State> {
        todo!()
    }

    #[instrument(skip(self))]
    pub async fn start_watching_saves(&self) -> Result<()> {
        let save_dir = Self::save_dir().unwrap();
        debug!(?save_dir);

        // let (tx, rx) = std::sync::mpsc::channel();
        // let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(250))?;
        // watcher.watch(save_dir, RecursiveMode::NonRecursive)?;
        //
        // // let s = self.clone();
        // tokio::spawn(async move {
        //     // Move watcher into here, since it would be dropped otherwise and then the channel
        //     // would be dropped.
        //     let _ = watcher;
        //
        //     trace!("Loop started.");
        //     loop {
        //         match rx.try_recv() {
        //             Ok(event) => {
        //                 info!(?event);
        //                 if let DebouncedEvent::Create(path) = event {
        //
        //                     let filename = path.file_name().unwrap().to_str().unwrap().into();
        //                     self.new_files_seen.push(filename);
        //                 }
        //             }
        //             Err(std::sync::mpsc::TryRecvError::Empty) => {}
        //             Err(std::sync::mpsc::TryRecvError::Disconnected) => {
        //                 warn!("Disconnected");
        //                 return;
        //             }
        //         }
        //
        //         tokio::task::yield_now().await;
        //     }
        // });
        todo!();

        Ok(())
    }

    pub fn process_new_saves(&mut self) -> Result<()> {
        let new_files = self.new_files_seen.clone();
        self.new_files_seen = vec![];

        for file in new_files {
            self.handle_save(&file).context(file).unwrap(); // TODO: unwrap
        }

        Ok(())
    }

    /// Example filename: Casimir III_0028 BC-2320.Civ5Save
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
    fn handle_save(&mut self, filename: &str) -> Result<bool> {
        let turn = Self::turn_from_filename(filename)?;
        let turn = match turn {
            Some(turn) => turn,
            None => return Ok(false),
        };

        let full_path = Self::save_dir()?.join(filename);
        trace!(?full_path);
        let mut fp = File::open(&full_path).context("Opening save")?;
        let mut bytes = Vec::with_capacity(1_000_000);
        fp.read_to_end(&mut bytes)?;
        drop(fp);
        let new_parsed_save = Civ5SaveReader::new(&bytes).parse()?;

        let info = self.find_game_for_save(&new_parsed_save)?.unwrap();
        let game_id = info.game.game_id;
        self.db
            .insert(Self::upload_bytes_db_key(&game_id), bytes)
            .unwrap();
        self.state.insert(game_id, State::UploadQueued);

        Ok(true)
    }

    #[instrument(skip(self))]
    pub async fn process_transfers(&mut self) -> Result<()> {
        if !self.user_ready() {
            return Ok(());
        }
        let my_games = self.my_games()?.clone();

        for info in my_games {
            let game_id = info.game.game_id;
            let state = {
                match self.state.get(&info.game.game_id) {
                    Some(s) => s.clone(),
                    None => State::Idle,
                }
            };

            trace!(?game_id, ?state);

            match state {
                State::Idle => self.process_idle_state(info).await?,
                State::Downloading => self.process_downloading_state(info).await?,
                State::Downloaded => {}
                State::UploadQueued => self.process_upload_queued(info).await?,
                // State::Uploading => self.handle_uploading(game).await?,
                // State::UploadComplete => self.handle_upload_complete(game).await?,
                _ => todo!("{:?}", state),
            }
        }
        Ok(())
    }

    #[instrument(skip(self, info))]
    async fn process_idle_state(&mut self, info: GameInfo) -> Result<()> {
        if info.game.current_turn.is_first_turn {
            // No save for first turn.
            return Ok(());
        }

        let path = Self::save_dir()?.join(Self::filename(&info.game)?);
        trace!(?path, "Downloading.");
        let (rx, handle) = self
            .api()?
            .get_latest_save_file_bytes(&info.game.game_id, &path)
            .await?;

        self.state.insert(info.game.game_id, State::Downloading);
        self.download_rx.insert(info.game.game_id, rx);
        Ok(())
    }

    #[instrument(skip(self, info))]
    async fn process_downloading_state(&mut self, info: GameInfo) -> Result<()> {
        let game_id = &info.game.game_id;
        let turn_id = &info.game.current_turn.turn_id;

        let rx = self.download_rx.get_mut(game_id).unwrap();
        let mut completed_download = None;
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
                    // Use update_state variable because we need to modify
                    // `self.download_state` which is currently borrowed.
                    completed_download = Some(path);
                    break;
                }
            }
        }
        if let Some(path) = completed_download {
            // Save the file into the DB because:
            // 1) The user might delete the file in the future
            // 2) Be able to analyse the file and compare when the user uploads their turn.
            self.store_downloaded_save(&game_id, &turn_id, &path)
                .unwrap();
            self.download_rx.remove(&game_id);
        }
        Ok(())
    }

    #[instrument(skip(self, info))]
    async fn process_upload_queued(&mut self, info: GameInfo) -> Result<()> {
        let game_id = info.game.game_id;
        let turn_id = info.game.current_turn.turn_id;
        info!(?game_id);

        self.state.insert(game_id, State::Uploading);

        todo!();
        // let s = self.clone();
        // tokio::spawn(async move {
        //     // TODO: Second unwrap is for an empty entry.
        //     // We're assuming the key exists if we've gone into this state.
        //     let bytes =
        //         s.db.get(Self::upload_bytes_db_key(&game_id))
        //             .unwrap()
        //             .unwrap();
        //
        //     info!(?game_id, ?turn_id, "Uploading.");
        //     let rx = s
        //         .api()
        //         .unwrap()
        //         .upload_save_client(turn_id, bytes.to_vec())
        //         .await
        //         .unwrap();
        // });

        Ok(())
    }

    fn find_game_for_save(&self, new_parsed_save: &Civ5Save) -> Result<Option<GameInfo>> {
        let mut smallest_diff = None;
        for info in self.my_games()? {
            let game_id = info.game.game_id;
            let new_turn = new_parsed_save.header.turn;
            let span = trace_span!("diff", ?game_id, ?new_turn);
            let _enter = span.enter();

            // XXX: The turn in the filename doesn't match the API's turn.
            // let other_turn = info.game.current_turn.number;
            // if other_turn != turn && other_turn != turn + 1 {
            //     trace!(other_turn, turn, "Turn doesn't match.");
            //     continue;
            // }
            // trace!(other_turn, turn, "Turn matches!");

            let last_parsed_save = match &info.parsed {
                Some(other_parsed) => other_parsed,
                None => {
                    warn!("Save not parsed. Can not check.");
                    continue;
                }
            };
            let last_turn = last_parsed_save.header.turn;

            if new_turn != last_turn && new_turn != last_turn + 1 {
                trace!(
                    ?new_turn,
                    ?last_turn,
                    "Save game turns aren't close enough."
                );
                continue;
            }

            let diff = new_parsed_save.difference_score(&last_parsed_save)?;
            trace!(diff);
            smallest_diff = match smallest_diff {
                Some((sd, sd_info)) => {
                    if diff < sd {
                        Some((diff, info.clone()))
                    } else {
                        Some((sd, sd_info))
                    }
                }
                None => Some((diff, info.clone())),
            };
        }
        match smallest_diff {
            Some((_, sd_info)) => {
                info!(game_id = ?sd_info, "Smallest diff found.");
                Ok(Some(sd_info))
            }
            None => {
                warn!("No games found to compare.");
                Ok(None)
            }
        }
    }

    /// Returns Ok(None) when the filename is invalid.
    fn turn_from_filename(filename: &str) -> Result<Option<u64>> {
        // TODO: once_cell
        let re = Regex::new(r"(?P<leader>.*?)_(?P<turn>\d{4}) (?P<year>.*?)\.Civ5Save").unwrap();
        let captures = match re.captures(&filename) {
            None => return Ok(None),
            Some(captures) => captures,
        };
        trace!(?captures);
        let turn = captures.name("turn").unwrap().as_str();
        let turn: u64 = turn.parse().unwrap();
        Ok(Some(turn))
    }

    pub fn load_config(&mut self) -> Result<()> {
        let config = match self.db.get(CONFIG_KEY).context("Loading CONFIG_KEY.")? {
            Some(b) => serde_json::from_slice(&b).with_context(|| {
                let s = String::from_utf8(b.to_vec()).unwrap();
                format!("Parsing JSON: {}", s)
            })?,
            None => Config::default(),
        };
        self.config = config;
        Ok(())
    }

    // RwLockWriteGuard is used here so that a config field can be modified within the same
    // write lock as the caller.
    fn save_config(&self) -> Result<()> {
        let encoded = serde_json::to_vec(&self.config)?;
        self.db.insert(CONFIG_KEY, encoded.as_slice())?;
        Ok(())
    }

    pub fn save_auth_key(&mut self, key: &str) -> Result<()> {
        self.config.auth_key = Some(key.to_owned());
        self.save_config()?;
        Ok(())
    }

    #[instrument(skip(self))]
    pub fn load_games(&mut self) -> Result<()> {
        let data = match self.db.get(GAME_API_RESPONSE_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => GetGamesAndPlayers::default(),
        };
        trace!(?data, "Existing games in db.");
        self.games = data;

        for game in self.games.games.clone().into_iter().map(|g| g.clone()) {
            let game_id = game.game_id;
            let turn_id = game.current_turn.turn_id;

            if self
                .db
                .contains_key(Self::saved_bytes_db_key(&game_id, &turn_id))?
            {
                trace!(?game_id, "Marking game as already downloaded.");
                self.state.insert(game_id, State::Downloaded);
                let data = self
                    .db
                    .get(Self::saved_bytes_db_key(&game_id, &turn_id))
                    .unwrap()
                    .unwrap();
                self.analyse_download(&game_id, &data)?;
            }
        }

        Ok(())
    }

    pub fn save_games(&mut self, data: &GetGamesAndPlayers) -> Result<()> {
        let encoded = serde_json::to_vec(&data)?;
        self.db.insert(GAME_API_RESPONSE_KEY, encoded.as_slice())?;
        self.games = data.clone();
        Ok(())
    }

    pub fn clear_games(&self) -> Result<()> {
        self.db.remove(GAME_API_RESPONSE_KEY)?;
        Ok(())
    }

    fn api(&self) -> Result<Api> {
        match &self.config.auth_key {
            Some(auth_key) => Ok(Api::new(auth_key)),
            None => Err(anyhow!("Attempt to access API without auth key.")),
        }
    }
}

fn project_dirs() -> anyhow::Result<ProjectDirs> {
    Ok(ProjectDirs::from("", "civ.fun", "gmr").context("Could not determine ProjectDirs.")?)
}

pub(crate) fn data_dir_path(join: &Path) -> anyhow::Result<PathBuf> {
    Ok(project_dirs()?.data_dir().join(join))
}
