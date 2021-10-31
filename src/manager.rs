use crate::api::{
    Api, DownloadMessage, Game, GameId, GetGamesAndPlayers, Player, TurnId, UploadMessage, UserId,
};
use anyhow::Context;
use anyhow::{anyhow, Error};
use civ5save::{Civ5Save, Civ5SaveReader};
use directories::{BaseDirs, ProjectDirs};
use iced::futures::TryFutureExt;
use notify::{DebouncedEvent, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sled::IVec;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, RwLockWriteGuard};
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::Receiver;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, instrument, trace, trace_span, warn};

type Result<T> = anyhow::Result<T>;

const CONFIG_KEY: &str = "config";
const GAMES_KEY: &str = "games";
const AUTH_KEY: &str = "auth-key";
const USER_ID_KEY: &str = "user-id";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPlayer {
    player: Player,
    image_data: Vec<u8>,
    last_downloaded: SystemTime,
}

#[derive(Debug)]
pub enum TransferState {
    Idle,
    Downloading(Receiver<DownloadMessage>),
    Downloaded,
    UploadQueued,
    Uploading,
    UploadComplete,
}

#[derive(Debug)]
pub enum Event {
    AuthenticationSuccess,
    AuthenticationFailure,
    UpdatedGames(Vec<Game>),
    UpdatedPlayer(StoredPlayer),
}

#[derive(Debug)]
enum FetchGames {
    Games(Vec<Game>),
    StoredPlayer(StoredPlayer),
}

#[derive(Debug)]
pub struct Manager {
    db: sled::Db,
    transfer: HashMap<GameId, TransferState>,
    auth_rx: Option<oneshot::Receiver<Option<UserId>>>,
    fetch_games_rx: Option<mpsc::Receiver<Result<FetchGames>>>,
    upload_rx: HashMap<GameId, Receiver<UploadMessage>>,
    watch_files_rx: Option<Receiver<String>>,
}

impl Manager {
    pub fn new(db: sled::Db) -> Self {
        Self {
            db,
            transfer: Default::default(),
            auth_rx: None,
            fetch_games_rx: None,
            // download_rx: Default::default(),
            upload_rx: Default::default(),
            watch_files_rx: None,
        }
    }

    // TODO: Turn this into a builder pattern so `start()` is a `build()` in a `ManagerBuilder`.
    #[instrument(skip(self))]
    pub fn start(&mut self) -> Result<()> {
        trace!("Setting up manager.");
        self.fill_transfer_states().context("Transfer states.")?;

        if let Some(auth_key) = self.auth_key()? {
            debug!("☑ Has auth key.");
            self.authenticate(&auth_key)?;
        }

        if self.user_id()?.is_some() {
            debug!("☑ Has user_id.");

            trace!("Fetching games on startup.");
            self.fetch_games().context("Fetching games on startup.")?;
        }

        self.start_watching_saves()?;

        Ok(())
    }

    #[instrument(skip(self))]
    pub fn process(&mut self) -> Result<Vec<Event>> {
        let mut events = vec![];
        if let Some(ref mut rx) = self.auth_rx {
            match rx.try_recv() {
                Ok(maybe_user_id) => {
                    if let Some(event) = self
                        .handle_auth_response(maybe_user_id)
                        .with_context(|| format!("Handling auth response: {:?}", &maybe_user_id))?
                    {
                        events.push(event);
                    }
                }
                Err(_) => {}
            };
        }

        let mut fetched = vec![];
        if let Some(ref mut rx) = self.fetch_games_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => {
                        trace!(?event);
                        fetched.push(event);
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        }

        for fetch in fetched {
            match fetch.context("Fetch games.")? {
                FetchGames::Games(games) => {
                    self.save_games(&games)?;
                    events.push(Event::UpdatedGames(games));
                }
                FetchGames::StoredPlayer(stored_player) => {
                    self.save_stored_player(&stored_player)?;
                    events.push(Event::UpdatedPlayer(stored_player));
                }
            };
        }

        self.process_transfers()?;
        self.process_new_saves()?;

        if events.len() > 0 {
            trace!(?events);
        }

        Ok(events)
    }

    #[instrument(skip(self))]
    pub fn games(&self) -> Result<Vec<Game>> {
        Ok(match self.db.get(GAMES_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => vec![],
        })
    }

    #[instrument(skip(self))]
    fn my_games(&self) -> Result<Vec<Game>> {
        let user_id = self
            .user_id()?
            .ok_or(anyhow!("my_games requested without a valid auth state."))?;

        Ok(self
            .games()?
            .into_iter()
            .filter(|g| g.is_user_id_turn(&user_id))
            .collect())
    }

    #[instrument(skip(self, key))]
    pub fn authenticate(&mut self, key: &str) -> Result<()> {
        trace!("Authentication requested.");
        let (tx, rx) = oneshot::channel();
        self.auth_rx = Some(rx);
        self.save_auth_key(key)?;
        let api = self.api()?;

        tokio::spawn(async move {
            trace!("Sending authentication request.");
            let maybe_user_id = api.authenticate_user().await.unwrap();
            debug!(?maybe_user_id, "User ID response.");
            tx.send(maybe_user_id).unwrap();
        });

        Ok(())
    }

    #[instrument(skip(self))]
    fn handle_auth_response(&mut self, maybe_user_id: Option<UserId>) -> Result<Option<Event>> {
        trace!("Handling auth response.");

        let previous_user_id = self.user_id()?;
        if let Some(user_id) = maybe_user_id {
            self.save_user_id(&user_id)?;
            let mut should_clear = false;

            if let Some(previous_user_id) = previous_user_id {
                if previous_user_id != user_id {
                    info!("Clearing games because user_id is different");
                    self.clear_games().context("Clear games.")?;
                }
            }

            Ok(Some(Event::AuthenticationSuccess))
        } else {
            warn!("Failed to authenticate.");
            Ok(Some(Event::AuthenticationFailure))
        }
    }

    /// This will eventually fetch a second time if the players shown don't exist in the db.
    #[instrument(skip(self))]
    pub fn fetch_games(&mut self) -> Result<()> {
        trace!("Fetching games.");
        let (mut tx, rx) = mpsc::channel(5);
        self.fetch_games_rx = Some(rx);
        let api = self.api()?;
        let db = self.db.clone();
        tokio::spawn(async move {
            if let Err(err) = Self::do_fetch_games(db, api, &mut tx).await {
                tx.send(Err(err)).await.unwrap();
            }
        });
        Ok(())
    }

    async fn do_fetch_games(
        db: sled::Db,
        api: Api,
        tx: &mut mpsc::Sender<Result<FetchGames>>,
    ) -> Result<()> {
        let games = api.get_games_and_players(&[]).await?;
        tx.send(Ok(FetchGames::Games(games.games.clone())))
            .await
            .unwrap();

        let unknown_players =
            Self::filter_unknown_players(&db, &games).context("Filter unknown players.")?;
        if unknown_players.len() == 0 {
            return Ok(());
        }

        let data = api
            .get_games_and_players(unknown_players.as_slice())
            .await?;

        for player in data.players {
            debug!(avatar_url = ?player.avatar_url, "Fetching avatar.");
            let db_ = db.clone();
            let tx_ = tx.clone();
            let player = player.clone();
            tokio::spawn(async move {
                let result = Self::fetch_avatar(player, db_).await;
                tx_.send(result.map(|sp| FetchGames::StoredPlayer(sp)))
                    .await
                    .unwrap();
            });
        }

        Ok(())
    }

    // fn handle_fetch_games() {
    //     self.save_games(&games)?;
    //
    //     Ok(())
    // }

    #[instrument(skip(db))]
    async fn fetch_avatar(player: Player, db: sled::Db) -> Result<StoredPlayer> {
        let image_data = reqwest::get(&player.avatar_url)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .to_vec();

        let stored_player = StoredPlayer {
            player,
            image_data,
            last_downloaded: SystemTime::now(),
        };

        Ok(stored_player)
    }

    fn filter_unknown_players(db: &sled::Db, games: &GetGamesAndPlayers) -> Result<Vec<UserId>> {
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
            let data = db
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

    // pub fn user_id(&self) -> Option<UserId> {
    //     if let AuthState::AuthResult(maybe_id) = self.config.auth_state {
    //         maybe_id
    //     } else {
    //         None
    //     }
    // }
    //
    fn player_info_key(user_id: &UserId) -> String {
        format!("player-info-{}", user_id)
    }

    fn saved_bytes_db_key(game_id: &GameId, turn_id: &TurnId) -> String {
        format!("saved-bytes-{}-{}", game_id, turn_id)
    }

    fn analysed_game_key(game_id: &GameId, turn_id: &TurnId) -> String {
        format!("analysed-{}-{}", game_id, turn_id)
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
        self.transfer
            .insert(game_id.clone(), TransferState::Downloaded);

        self.analyse(game_id, turn_id, &data)?;

        Ok(())
    }

    #[instrument(skip(self, data))]
    fn analyse(&mut self, game_id: &GameId, turn_id: &TurnId, data: &[u8]) -> Result<()> {
        trace!(data_len = ?data.len(), "Analysing save.");
        let civ5save = Civ5SaveReader::new(&data).parse()?;
        trace!(?civ5save);

        let key = Self::analysed_game_key(game_id, turn_id);
        let encoded = serde_json::to_vec(&civ5save)?;
        self.db.insert(key, encoded)?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn analysed(&self, game_id: &GameId, turn_id: &TurnId) -> Result<Option<Civ5Save>> {
        let key = Self::analysed_game_key(game_id, turn_id);
        let bytes = self.db.get(key).context("Fetching analysed")?;
        match bytes {
            None => Ok(None),
            Some(b) => Ok(Some(serde_json::from_slice(&b)?)),
        }
    }

    pub fn download_status(&self) -> Vec<TransferState> {
        todo!()
    }

    #[instrument(skip(self))]
    pub fn start_watching_saves(&mut self) -> Result<()> {
        let save_dir = Self::save_dir().unwrap();
        debug!(?save_dir);

        let (tx, rx) = mpsc::channel(10);
        self.watch_files_rx = Some(rx);

        let (watch_tx, watch_rx) = std::sync::mpsc::channel();
        let mut watcher: RecommendedWatcher = Watcher::new(watch_tx, Duration::from_millis(250))?;
        watcher.watch(save_dir, RecursiveMode::NonRecursive)?;

        tokio::spawn(async move {
            // Move watcher into here, since it would be dropped otherwise and then the channel
            // would be dropped.
            let _ = watcher;

            trace!("Loop started.");
            loop {
                let event = watch_rx.try_recv();
                match event {
                    Ok(event) => {
                        info!(?event);
                        if let DebouncedEvent::Create(path) = event {
                            let filename = path.file_name().unwrap().to_str().unwrap().into();
                            tx.send(filename).await.unwrap();
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

    pub fn process_new_saves(&mut self) -> Result<()> {
        let rx = match self.watch_files_rx {
            Some(ref mut rx) => rx,
            None => {
                warn!("Receiver is None for watch_files_rx.");
                return Ok(());
            }
        };

        let mut found = vec![];
        while let Ok(file) = rx.try_recv() {
            found.push(file);
        }
        for file in found {
            self.handle_save(&file).context(file)?;
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

        let potential_games = self.find_game_for_save(&new_parsed_save)?;
        if potential_games.len() == 0 {
            todo!("New save file has no potential matches. Ask user about it?");
        } else if potential_games.len() == 1 {
            let game = &potential_games[0];
            let game_id = game.game_id;
            self.db
                .insert(Self::upload_bytes_db_key(&game_id), bytes)
                .unwrap();
            self.transfer.insert(game_id, TransferState::UploadQueued);
        } else {
            todo!("Multiple potential saves. Ask the user about it?");
        }

        Ok(true)
    }

    #[instrument(skip(self))]
    pub fn process_transfers(&mut self) -> Result<()> {
        for game in self.my_games()? {
            let game_id = game.game_id;

            let state = self
                .transfer
                .entry(game.game_id.clone())
                .or_insert(TransferState::Idle);

            trace!(?game_id, ?state);

            match state {
                TransferState::Idle => self.process_idle_state(game)?,
                // TransferState::Downloading() => self.process_downloading_state(game)?,
                TransferState::Downloaded => {}
                // TransferState::UploadQueued => self.process_upload_queued(game)?,
                // State::Uploading => self.handle_uploading(game)?,
                // State::UploadComplete => self.handle_upload_complete(game).await?,
                _ => todo!("{:?}", state),
            }
        }
        Ok(())
    }

    #[instrument(skip(self, game))]
    fn process_idle_state(&mut self, game: Game) -> Result<()> {
        if game.current_turn.is_first_turn {
            // No save for first turn.
            trace!("First turn. Marking as downloaded.");
            self.transfer
                .insert(game.game_id, TransferState::Downloaded);
            return Ok(());
        }

        let path = Self::save_dir()?.join(Self::filename(&game)?);
        trace!(?path, "Downloading.");
        let (rx, handle) = self
            .api()?
            .get_latest_save_file_bytes(&game.game_id, &path)?;

        self.transfer
            .insert(game.game_id, TransferState::Downloading(rx));
        Ok(())
    }

    // #[instrument(skip(self, game))]
    // async fn process_downloading_state(&mut self, game: Game) -> Result<()> {
    //     let game_id = &game.game_id;
    //     let turn_id = &game.current_turn.turn_id;
    //
    //     let rx = self.download_rx.get_mut(game_id).unwrap();
    //     let mut completed_download = None;
    //     loop {
    //         let msg = match rx.try_recv() {
    //             Ok(msg) => msg,
    //             Err(TryRecvError::Empty) => break,
    //             Err(err) => panic!("{:?}", err),
    //         };
    //         match msg {
    //             DownloadMessage::Error(e) => {
    //                 error!(?e, "Download");
    //             }
    //             DownloadMessage::Started(size) => {
    //                 trace!(?size, "Started");
    //             }
    //             DownloadMessage::Chunk(percentage) => {
    //                 trace!(?percentage, "Download progress");
    //             }
    //             DownloadMessage::Done(path) => {
    //                 trace!("Done!");
    //                 // Use update_state variable because we need to modify
    //                 // `self.download_state` which is currently borrowed.
    //                 completed_download = Some(path);
    //                 break;
    //             }
    //         }
    //     }
    //     if let Some(path) = completed_download {
    //         // Save the file into the DB because:
    //         // 1) The user might delete the file in the future
    //         // 2) Be able to analyse the file and compare when the user uploads their turn.
    //         self.store_downloaded_save(&game_id, &turn_id, &path)
    //             .unwrap();
    //         self.download_rx.remove(&game_id);
    //     }
    //     Ok(())
    // }

    #[instrument(skip(self, game))]
    async fn process_upload_queued(&mut self, game: Game) -> Result<()> {
        let game_id = game.game_id;
        let turn_id = game.current_turn.turn_id;
        info!(?game_id);

        self.transfer.insert(game_id, TransferState::Uploading);

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

    #[instrument(skip(self, new_parsed_save))]
    fn find_game_for_save(&self, new_parsed_save: &Civ5Save) -> Result<Vec<Game>> {
        let new_turn = new_parsed_save.header.turn;

        // We're at the first turn. Only look for games that GMR say is the first turn.
        let mut suspects = vec![];
        if new_turn == 0 {
            for game in self.my_games()? {
                if game.current_turn.is_first_turn {
                    suspects.push(game);
                }
            }
            return Ok(suspects);
        }

        let mut smallest_diff: Option<(u32, Game)> = None;
        for game in self.my_games()? {
            let game_id = &game.game_id;
            trace!(?game_id);

            // XXX: The turn in the filename doesn't match the API's turn.
            // let other_turn = info.game.current_turn.number;
            // if other_turn != turn && other_turn != turn + 1 {
            //     trace!(other_turn, turn, "Turn doesn't match.");
            //     continue;
            // }
            // trace!(other_turn, turn, "Turn matches!");

            let last_parsed = self.analysed(&game.game_id, &game.current_turn.turn_id)?;
            let last_parsed_save = match last_parsed {
                Some(parsed) => parsed,
                None => {
                    warn!(?game, "Skipping save because of no analysis.");
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
                Some((sd, game)) => {
                    if diff < sd {
                        Some((diff, game.clone()))
                    } else {
                        Some((sd, game))
                    }
                }
                None => Some((diff, game.clone())),
            };
        }

        match smallest_diff {
            Some((_, game)) => {
                info!(game_id = ?game.game_id, "Smallest diff found.");
                Ok(vec![game])
            }
            None => {
                warn!("No games found to compare.");
                Ok(vec![])
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

    /// This is private. Use `authenticate()` to set a key instead. It has extra logic for deleting
    /// existing state if the user has changed.
    fn save_auth_key(&self, key: &str) -> Result<()> {
        self.db.insert(AUTH_KEY, key)?;
        Ok(())
    }

    pub fn auth_key(&self) -> Result<Option<String>> {
        self.db
            .get(AUTH_KEY)?
            .map(|iv| String::from_utf8(iv.to_vec()).with_context(|| format!("Parsing {:?}", iv)))
            .transpose()
    }

    pub fn save_user_id(&self, user_id: &UserId) -> Result<()> {
        self.db
            .insert(USER_ID_KEY, format!("{}", user_id).as_str())?;
        Ok(())
    }

    pub fn user_id(&self) -> Result<Option<UserId>> {
        self.db
            .get(USER_ID_KEY)?
            .map(Self::decode_user_id)
            .transpose()
    }

    fn decode_user_id(iv: IVec) -> Result<UserId> {
        let context = || format!("Parsing {:?}", &iv);
        let s = String::from_utf8(iv.to_vec()).with_context(context)?;
        let n = s.parse::<u64>().with_context(context)?;
        Ok(n.into())
    }

    #[instrument(skip(self))]
    pub fn fill_transfer_states(&mut self) -> Result<()> {
        for game in self.games()? {
            let game_id = game.game_id;
            let turn_id = game.current_turn.turn_id;

            if self
                .db
                .contains_key(Self::saved_bytes_db_key(&game_id, &turn_id))?
            {
                trace!(?game_id, "Marking game as already downloaded.");
                self.transfer.insert(game_id, TransferState::Downloaded);
            }
        }

        Ok(())
    }

    pub fn save_games(&self, games: &[Game]) -> Result<()> {
        let encoded = serde_json::to_vec(games)?;
        self.db.insert(GAMES_KEY, encoded.as_slice())?;
        Ok(())
    }

    pub fn clear_games(&self) -> Result<()> {
        self.db.remove(GAMES_KEY)?;
        Ok(())
    }

    fn save_stored_player(&self, stored_player: &StoredPlayer) -> Result<()> {
        let key = Self::player_info_key(&stored_player.player.steam_id);
        let json = serde_json::to_vec(&stored_player).context("Encoding player info.")?;
        trace!(?key, ?json, "Saving player info.");
        self.db.insert(key, json).context("Saving player info.")?;
        Ok(())
    }

    fn api(&self) -> Result<Api> {
        match &self.auth_key()? {
            Some(auth_key) => Ok(Api::new(auth_key)),
            None => Err(anyhow!("Attempt to access API without auth key.")),
        }
    }
}

pub fn project_dirs() -> anyhow::Result<ProjectDirs> {
    Ok(ProjectDirs::from("", "civ.fun", "gmr").context("Could not determine ProjectDirs.")?)
}

pub fn data_dir_path(join: &Path) -> anyhow::Result<PathBuf> {
    Ok(project_dirs()?.data_dir().join(join))
}
