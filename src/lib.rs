mod api;
mod manager;

use anyhow::Context;
pub use api::Api;
use directories::ProjectDirs;
pub use manager::Manager;
use std::path::{Path, PathBuf};

fn project_dirs() -> anyhow::Result<ProjectDirs> {
    Ok(
        ProjectDirs::from("fun.civ", "civ.fun", "civ.fun gmr client")
            .context("Could not determine ProjectDirs")?,
    )
}

pub(crate) fn data_dir_path(join: &Path) -> anyhow::Result<PathBuf> {
    Ok(project_dirs()?.data_dir().join(join))
}
