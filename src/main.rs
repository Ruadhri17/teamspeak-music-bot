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

use std::{fs::File, path::PathBuf, time::Duration};

use clap::Parser;

use audio::AudioInput;
use cli::{Cli, Command};
use client::{Client, ClientEvent, ClientRequest};
use config::{AudioConfig, ConfigLocation, LogLevel};
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
    let (frames_tx, frames_rx) = mpsc::channel(250);
    let client = Client::new(
        &config.teamspeak,
        events_tx,
        requests_rx,
        frames_rx,
        shutdown.clone(),
    )?;
    let mut client = tokio::spawn(client.run());
    tokio::spawn(ping(
        events_rx,
        requests_tx,
        frames_tx,
        config.paths.ffmpeg_path.clone(),
        config.audio.clone(),
    ));

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

/// Temporary stand-in for commands and the player: `!bot ping` checks both channels,
/// `!bot play <path>` plays a local file.
// TODO: remove once `commands` and the player handle messages.
async fn ping(
    mut events: mpsc::Receiver<ClientEvent>,
    requests: mpsc::Sender<ClientRequest>,
    frames: mpsc::Sender<Vec<u8>>,
    ffmpeg_path: PathBuf,
    audio: AudioConfig,
) {
    // Cancelled when the next `!bot play` replaces the current track.
    let mut stop = CancellationToken::new();

    while let Some(event) = events.recv().await {
        let ClientEvent::Message {
            reply_to,
            sender_name,
            text,
        } = event;
        let text = text.trim();

        let answer = if text == "!bot ping" {
            info!("ping from {sender_name}");
            "pong".to_string()
        } else if let Some(path) = text.strip_prefix("!bot play ") {
            // TODO: any path on the host can be played from the chat, this must never reach
            //   the VPS. `sources/local` will only accept files inside `music_dir`.
            match File::open(path.trim()) {
                Ok(file) => {
                    info!("{sender_name} plays {path}");
                    // The old thread stops at its next frame, frames already buffered still play.
                    stop.cancel();
                    stop = CancellationToken::new();
                    let input: AudioInput = Box::new(file);
                    audio::play(
                        ffmpeg_path.clone(),
                        input,
                        &audio,
                        frames.clone(),
                        stop.clone(),
                    );
                    // Success needs no answer, every chat message notifies the whole channel.
                    continue;
                }
                Err(err) => format!("cannot open {path}: {err}"),
            }
        } else {
            continue;
        };

        let reply = ClientRequest::SendMessage {
            to: reply_to,
            text: answer,
        };
        // `Err` means the client is gone, so there is nobody left to answer.
        if requests.send(reply).await.is_err() {
            break;
        }
    }
}
