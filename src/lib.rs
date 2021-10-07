mod api;
mod manager;

pub use api::Api;
pub use manager::{Config, Manager};

use anyhow::Context;
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

fn project_dirs() -> anyhow::Result<ProjectDirs> {
    Ok(ProjectDirs::from("", "civ.fun", "gmr").context("Could not determine ProjectDirs.")?)
}

pub(crate) fn data_dir_path(join: &Path) -> anyhow::Result<PathBuf> {
    Ok(project_dirs()?.data_dir().join(join))
}
