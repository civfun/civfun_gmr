use crate::{data_dir_path, project_dirs};
use anyhow::anyhow;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::debug;

const CONFIG_KEY: &str = "config";

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    auth_key: Option<String>,
}

#[derive(Debug)]
pub struct Manager {
    db: sled::Db,
}

impl Manager {
    pub fn new() -> anyhow::Result<Self> {
        let db_path =
            data_dir_path(&PathBuf::from("db.sled")).context("Constructing db.sled path")?;
        debug!("DB path: {:?}", &db_path);
        let db = sled::open(&db_path)
            .with_context(|| format!("Could not create db at {:?}", &db_path))?;
        Ok(Self { db })
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
        Ok(())
    }
}
