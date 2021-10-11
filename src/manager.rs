use crate::api::{Api, Game, GameId, GetGamesAndPlayers, UserId};
use crate::{data_dir_path, project_dirs};
use anyhow::anyhow;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, RwLockWriteGuard};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tracing::{debug, instrument};

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

#[derive(Debug, Default)]
struct Inner {
    config: Config,
    games: GetGamesAndPlayers,
    downloads: HashMap<GameId, Download>,
}

#[derive(Debug)]
pub enum Download {
    Idle,
    Downloading(JoinHandle<()>, Receiver<()>),
    Complete,
}

#[derive(Debug, Clone)]
pub struct Manager {
    db: sled::Db,
    inner: Arc<RwLock<Inner>>,
}

impl Manager {
    pub fn new() -> anyhow::Result<Self> {
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
    pub async fn authenticate(&mut self) -> anyhow::Result<Option<UserId>> {
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
    pub async fn refresh(&mut self) -> anyhow::Result<()> {
        let games = self.api()?.get_games_and_players(&[]).await?;
        dbg!("???? 1");
        self.save_games(games)?;
        // TODO
        // let unknown_players = self.filter_unknown_players();
        // if unknown_players.len() > 0 {
        //     let data = self.api()?.get_games_and_players([]).await?;
        // }
        dbg!("???? 2");
        self.start_downloads().await;
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

    pub fn get_download_info(&self, game_id: &GameId) -> &Download {
        // let inner = self.inner.read().unwrap();
        // // TODO: unwrap
        // inner.downloads.get(&game_id).unwrap()
        todo!()
    }

    pub async fn start_downloads(&mut self) -> anyhow::Result<Option<()>> {
        // If we don't have a user_id, don't bother trying.
        dbg!("???? xxxx");
        let user_id = match self.user_id() {
            None => return Ok(None),
            Some(u) => u,
        };

        let inner = self.inner.read().unwrap();
        dbg!("????");
        for game in &inner.games.games {
            if !game.is_user_id_turn(&user_id) {
                continue;
            }

            // let info = self.get_download_info(&game.game_id);
            // if let Download::Downloading(..) = info {
            //     continue;
            // }
            // dbg!(info);
            // let game_id = game.game_id.clone();
            // let (rx, handle) = self
            //     .api()?
            //     .get_latest_save_file_bytes(&game_id)
            //     .await
            //     .unwrap();

            self.api()?.get_latest_save_file_bytes(&game_id).await;

            // TODO: unwrap
        }

        Ok(Some(())) // TODO: return something useful?
    }

    pub fn download_status(&self) -> Vec<Download> {
        todo!()
    }

    pub fn load_config(&mut self) -> anyhow::Result<()> {
        let config = match self.db.get(CONFIG_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => Config::default(),
        };
        self.inner.write().unwrap().config = config;
        Ok(())
    }

    // RwLockWriteGuard is used here so that a config field can be modified within the same
    // write lock as the caller.
    fn save_config(&self, inner: &mut RwLockWriteGuard<Inner>) -> anyhow::Result<()> {
        let encoded = serde_json::to_vec(&inner.config)?;
        self.db.insert(CONFIG_KEY, encoded.as_slice())?;
        Ok(())
    }

    pub fn save_auth_key(&mut self, key: &str) -> anyhow::Result<()> {
        let mut inner = self.inner.write().unwrap();
        inner.config.auth_key = Some(key.to_owned());
        self.save_config(&mut inner)?;
        Ok(())
    }

    pub fn load_games(&mut self) -> anyhow::Result<()> {
        let data = match self.db.get(GAME_API_RESPONSE_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => GetGamesAndPlayers::default(),
        };

        self.inner.write().unwrap().games = data;
        Ok(())
    }

    pub fn save_games(&self, data: GetGamesAndPlayers) -> anyhow::Result<()> {
        let mut inner = self.inner.write().unwrap();
        let encoded = serde_json::to_vec(&data)?;
        self.db.insert(GAME_API_RESPONSE_KEY, encoded.as_slice())?;
        inner.games = data;
        Ok(())
    }

    pub fn clear_games(&self) -> anyhow::Result<()> {
        self.db.remove(GAME_API_RESPONSE_KEY)?;
        Ok(())
    }

    fn api(&self) -> anyhow::Result<Api> {
        let inner = self.inner.read().unwrap();
        match &inner.config.auth_key {
            Some(auth_key) => Ok(Api::new(auth_key)),
            None => Err(anyhow!("Attempt to access API without auth key.")),
        }
    }

    pub fn games(&self) -> Vec<Game> {
        self.inner.read().unwrap().games.games.clone()
    }
}
