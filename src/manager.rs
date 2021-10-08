use crate::api::{Api, Game, GetGamesAndPlayers};
use crate::{data_dir_path, project_dirs};
use anyhow::anyhow;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, instrument};

const CONFIG_KEY: &str = "config";
const DATA_KEY: &str = "data";

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    auth_key: Option<String>,
    user_id: Option<String>,
}

#[derive(Debug)]
pub struct Manager {
    db: sled::Db,
    config: Config,
    data: GetGamesAndPlayers,
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
            config: Default::default(),
            data: Default::default(),
        };
        s.load_config()?;
        s.load_data()?;
        Ok(s)
    }

    #[instrument(skip(self))]
    pub async fn authenticate(&mut self) -> anyhow::Result<()> {
        let user_id = self.api()?.authenticate_user().await?;
        debug!("User ID: {}", user_id);
        self.config.user_id = Some(user_id);
        self.save_config();
        Ok(())
    }

    /// Ready means we have an auth key and a user id.
    pub fn all_ready(&self) -> bool {
        self.auth_ready() && self.config.user_id.is_some()
    }

    pub fn auth_ready(&self) -> bool {
        self.config.auth_key.is_some()
    }

    /// This will eventually fetch a second time if the players shown don't exist in the db.
    /// It will also start downloading games if they don't exist.
    pub async fn refresh(&mut self) -> anyhow::Result<()> {
        let data = self.api()?.get_games_and_players(&[]).await?;
        self.save_data(&data)?;
        Ok(())
    }

    pub fn load_config(&mut self) -> anyhow::Result<()> {
        self.config = match self.db.get(CONFIG_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => Config::default(),
        };
        Ok(())
    }

    pub fn save_config(&self) -> anyhow::Result<()> {
        let encoded = serde_json::to_vec(&self.config)?;
        self.db.insert(CONFIG_KEY, encoded.as_slice())?;
        Ok(())
    }

    pub fn save_auth_key(&mut self, key: &str) -> anyhow::Result<()> {
        self.config.auth_key = Some(key.to_owned());
        self.save_config()?;
        Ok(())
    }

    pub fn load_data(&mut self) -> anyhow::Result<()> {
        let data = match self.db.get(DATA_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => GetGamesAndPlayers::default(),
        };

        self.data = data;
        Ok(())
    }

    pub fn save_data(&self, data: &GetGamesAndPlayers) -> anyhow::Result<()> {
        let encoded = serde_json::to_vec(data)?;
        self.db.insert(DATA_KEY, encoded.as_slice())?;
        Ok(())
    }

    pub fn clear_data(&self) -> anyhow::Result<()> {
        self.db.remove(DATA_KEY)?;
        Ok(())
    }

    fn api(&self) -> anyhow::Result<Api> {
        match &self.config.auth_key {
            Some(auth_key) => Ok(Api::new(auth_key)),
            None => Err(anyhow!("Attempt to access API without auth key.")),
        }
    }

    pub fn games(&self) -> &Vec<Game> {
        &self.data.games
    }

    pub fn reorder_games(&mut self) {
        // self.latest_data.games.sort_by(|a, b| {
        // })
    }
}
