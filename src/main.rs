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
use client::{Client, ClientEvent, ClientRequest};
use config::{ConfigLocation, LogLevel};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::mpsc,
};
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

    let (events_tx, events_rx) = mpsc::channel(64);
    let (requests_tx, requests_rx) = mpsc::channel(64);
    let client = Client::new(&config.teamspeak, events_tx, requests_rx, shutdown.clone())?;
    let mut client = tokio::spawn(client.run());
    tokio::spawn(ping(events_rx, requests_tx));

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

/// Temporary stand-in for commands and the player, checks that both channels work.
// TODO: remove once `commands` and the player handle messages.
async fn ping(mut events: mpsc::Receiver<ClientEvent>, requests: mpsc::Sender<ClientRequest>) {
    while let Some(event) = events.recv().await {
        let ClientEvent::Message {
            reply_to,
            sender_name,
            text,
        } = event;
        if text.trim() != "!bot ping" {
            continue;
        }

        info!("ping from {sender_name}");
        let pong = ClientRequest::SendMessage {
            to: reply_to,
            text: "pong".into(),
        };
        // `Err` means the client is gone, so there is nobody left to answer.
        if requests.send(pong).await.is_err() {
            break;
        }
    }
}
