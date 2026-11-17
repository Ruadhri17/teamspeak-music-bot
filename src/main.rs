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
use client::{AudioControl, Client};
use config::{ConfigLocation, LogLevel};
use player::{AudioLauncher, Player};
use secrets::Secrets;
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::{mpsc, watch},
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

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

    let secrets = Secrets::load(&config.paths.secrets_file)?;

    let shutdown = CancellationToken::new();

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    let (events_tx, events_rx) = mpsc::channel(64);
    let (requests_tx, requests_rx) = mpsc::channel(64);
    let (frames_tx, frames_rx) = mpsc::channel(250);

    let (control_tx, control_rx) = watch::channel(AudioControl::default());

    let client = Client::new(
        &config,
        &secrets.teamspeak,
        events_tx,
        requests_rx,
        frames_rx,
        control_rx,
        shutdown.clone(),
    )?;
    let mut client = tokio::spawn(client.run());
    let launcher = Box::new(AudioLauncher::new(&config, frames_tx));
    let player = Player::new(events_rx, requests_tx, control_tx, launcher, &config);
    // Not awaited, the player stops when the client exits and the events channel closes.
    tokio::spawn(player.run());

    tokio::select! {
        _ = sigint.recv() => info!("SIGINT received, shutting down"),
        _ = sigterm.recv() => info!("SIGTERM received, shutting down"),
        result = &mut client => return Ok(result??),
    }

    shutdown.cancel();
    match tokio::time::timeout(Duration::from_secs(5), client).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(err))) => warn!(
            "client failed during shutdown: {:#}",
            anyhow::Error::from(err)
        ),
        Ok(Err(err)) => warn!("client task panicked during shutdown: {err}"),
        Err(_) => warn!("client did not stop in 5 seconds, exiting anyway"),
    }

    Ok(())
}
