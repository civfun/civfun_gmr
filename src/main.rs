use clap::{AppSettings, Clap};
use giant_multiplayer_robot::Manager;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts: Opts = Opts::parse();
    // let gmr = Client::new(&opts.auth_key);
    // dbg!(gmr.get_games_and_players().await.unwrap());

    let gmr = Manager::new()?;
    let config = gmr.has_config()?;
    dbg!(&config);
    // let games = gmr.games().await?;
    // gmr.download(games[0].game_id).await?;
    // let path = gmr.check_for_new_save().await?;

    Ok(())
}
