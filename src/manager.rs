use crate::api::{Api, Game, GetGamesAndPlayers};
use crate::{data_dir_path, project_dirs};
use anyhow::anyhow;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::debug;

const CONFIG_KEY: &str = "config";

/// Player and Game data stored in here.
const DATA_KEY: &str = "data";

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    auth_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Manager {
    db: sled::Db,
    latest_data: Option<GetGamesAndPlayers>,
}

impl Manager {
    pub fn new() -> anyhow::Result<Self> {
        let db_path =
            data_dir_path(&PathBuf::from("db.sled")).context("Constructing db.sled path")?;
        debug!("DB path: {:?}", &db_path);
        let db = sled::open(&db_path)
            .with_context(|| format!("Could not create db at {:?}", &db_path))?;

        let s = Self {
            db,
            latest_data: None,
        };
        // let config = s.get_or_create_config()?;
        // let data = s.load_data()?;

        Ok(s)
    }

    pub fn get_or_create_config(&self) -> anyhow::Result<Config> {
        Ok(match self.db.get(CONFIG_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => Config::default(),
        })
    }

    pub fn save_config(&self, config: &Config) -> anyhow::Result<()> {
        let encoded = serde_json::to_vec(config)?;
        self.db.insert(CONFIG_KEY, encoded.as_slice())?;
        Ok(())
    }

    pub fn set_auth_key(&self, key: &str) -> anyhow::Result<()> {
        let mut config = self.get_or_create_config()?;
        config.auth_key = Some(key.to_owned());
        self.save_config(&config)?;
        Ok(())
    }

    pub fn has_auth_key(&self) -> anyhow::Result<bool> {
        Ok(self.get_or_create_config()?.auth_key.is_some())
    }

    pub fn get_auth_key(&self) -> anyhow::Result<String> {
        match self.get_or_create_config()?.auth_key {
            None => Err(anyhow!("auth_key not set while fetching.")),
            Some(k) => Ok(k),
        }
    }

    pub fn load_data(&mut self) -> anyhow::Result<()> {
        let data = match self.db.get(DATA_KEY)? {
            Some(b) => serde_json::from_slice(&b)?,
            None => GetGamesAndPlayers::default(),
        };

        self.latest_data = Some(data);
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

    fn latest(&self) -> anyhow::Result<&GetGamesAndPlayers> {
        todo!()
        // if let Some(ref data) = self.latest_data {
        //     Ok(data)
        // } else {
        // }
    }

    fn api(&self) -> anyhow::Result<Api> {
        Ok(Api::new(&self.get_auth_key()?))
    }

    /// This will eventually fetch a second time if the players shown don't exist in the db.
    /// It will also start downloading games if they don't exist.
    pub async fn refresh(&mut self) -> anyhow::Result<()> {
        let data = self.api()?.get_games_and_players(&[]).await?;
        self.save_data(&data)?;
        Ok(())
    }
}
