use crate::{data_dir_path, project_dirs};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_KEY: &str = "config";

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Config {
    auth_key: Option<String>,
}

pub struct Manager {
    db: sled::Db,
}

impl Manager {
    pub fn new() -> anyhow::Result<Self> {
        let db_path =
            data_dir_path(&PathBuf::from("db.sled")).context("Constructing db.sled path")?;
        let db = sled::open(&db_path)
            .with_context(|| format!("Could not create db at {:?}", &db_path))?;
        Ok(Self { db })
    }

    pub fn has_config(&self) -> anyhow::Result<bool> {
        Ok(self.db.get(CONFIG_KEY)?.is_some())
    }
}
