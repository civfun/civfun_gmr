use clap::{AppSettings, Clap};

mod style;
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

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // let opts: Opts = Opts::parse();
    // let gmr = Client::new(&opts.auth_key);
    // dbg!(gmr.get_games_and_players().await.unwrap());
    // let manager = Manager::new()?;
    // let config = manager.get_or_create_config()?;
    // dbg!(&config);
    // let games = gmr.games().await?;
    // gmr.download(games[0].game_id).await?;
    // let path = gmr.check_for_new_save().await?;

    ui::run()?;

    Ok(())
}
