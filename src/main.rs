//! Entry point: CLI (`init`, `run`, `tidal-login`), loading config and secrets,
//! starting the tasks (TS client, player, presence) and wiring them with channels.

mod audio;
mod cli;
mod client;
mod commands;
mod config;
mod logging;
mod permissions;
mod player;
mod presence;
mod secrets;
mod sources;

use std::time::Duration;

use clap::Parser;

use cli::{Cli, Command};
use config::{ConfigLocation, LogLevel};
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Returning `Err` prints it with all causes ("Caused by:") and exits with code 1.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let location = ConfigLocation::new(cli.config)?;

    match cli.command {
        Command::Init { force } => init(&location, force),
        Command::Run => run(&location, cli.log_level).await,
    }
}

fn init(location: &ConfigLocation, force: bool) -> anyhow::Result<()> {
    location.init(force)?;
    println!("config file created at: {}", location.file_path().display());
    Ok(())
}

async fn run(location: &ConfigLocation, log_level: Option<LogLevel>) -> anyhow::Result<()> {
    let config = location.load()?;
    logging::init(log_level, config.logging.level);
    info!("using config file {}", location.file_path().display());

    let shutdown = CancellationToken::new();

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut client = tokio::spawn(client::connect());

    // TODO: pass `&config.teamspeak` to the client instead of the hardcoded address.
    // TODO: secrets, identity.
    tokio::select! {
        _ = sigint.recv() => info!("SIGINT received, shutting down"),
        _ = sigterm.recv() => info!("SIGTERM received, shutting down"),
        result = &mut client => return Ok(result?),
    }

    shutdown.cancel();
    match tokio::time::timeout(Duration::from_secs(5), client).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => warn!("client failed during shutdown: {err}"),
        Err(_) => warn!("client did not stop in 5 seconds, exiting anyway"),
    }

    Ok(())
}
