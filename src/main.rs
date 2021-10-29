use anyhow::Context;
use civfun_gmr::manager::{data_dir_path, Manager};
use clap::{AppSettings, Clap};
use std::path::PathBuf;
use tracing::debug;

mod ui;

pub const TITLE: &str = "civ.fun's Multiplayer Robot";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clap)]
#[clap(setting = AppSettings::ColoredHelp)]
struct Opts {
    #[clap(env = "GMR_AUTH_KEY")]
    auth_key: String,
    // cmd: SubCommand,
}

#[derive(Clap)]
enum SubCommand {
    // Login(LoginOpts),
// List(ListOpts),
// Download(DownloadOpts),
// Submit(SubmitOpts),
}

fn main() {
    run().unwrap();
}

fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // let opts: Opts = Opts::parse();

    let db_path = data_dir_path(&PathBuf::from("db.sled")).context("Constructing db.sled path")?;
    debug!(?db_path);

    let db =
        sled::open(&db_path).with_context(|| format!("Could not create db at {:?}", &db_path))?;
    let mut manager = Manager::new(db);
    manager.start()?;
    ui::run(manager)
}
