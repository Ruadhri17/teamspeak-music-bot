//! Entry point: CLI (`init`, `run`, `yt-check`), loading config and secrets,
//! starting the tasks (TS client, player, presence) and wiring them with channels.

mod audio;
mod chat;
mod cli;
mod client;
mod commands;
mod config;
mod logging;
mod permissions;
mod player;
mod presence;
mod process;
mod secrets;
mod sources;
mod yt_check;

use std::{
    path::Path,
    process::{Command as Process, Stdio},
    time::Duration,
};

use clap::Parser;

use cli::{Cli, Command};
use client::{AudioControl, Client};
use config::{Config, ConfigLocation, LogLevel};
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
        Command::YtCheck { query, full } => {
            yt_check(&location, cli.log_level, &query.join(" "), full).await
        }
    }
}

fn init(location: &ConfigLocation, force: bool) -> anyhow::Result<()> {
    location.init(force)?;
    println!("config file created at: {}", location.file_path().display());
    Ok(())
}

async fn yt_check(
    location: &ConfigLocation,
    log_level: Option<LogLevel>,
    query: &str,
    full: bool,
) -> anyhow::Result<()> {
    let config = location.load()?;
    // `warn` and up by default, so the yt-dlp errors show up between the results.
    logging::init(log_level.or(Some(LogLevel::Warn)), config.logging.level);
    let versions = check_dependencies(&config)?;
    println!("dependencies");
    for (name, version) in versions {
        println!("  {name:<7} {version}");
    }
    yt_check::run(&config, query, full).await
}

/// An external program the bot runs.
struct Program<'a> {
    name: &'static str,
    path: &'a Path,
    version_flag: &'static str,
    /// Where the path comes from, shown when the program is missing.
    origin: String,
}

/// Runs `--version` of every external program, so a missing one stops the bot at start,
/// not at the first `!song add`. Returns the version each program printed.
/// deno has no config key: `yt-dlp` looks for it in `PATH`, so this checks the same place.
fn check_dependencies(config: &Config) -> anyhow::Result<Vec<(&'static str, String)>> {
    let yt_dlp = &config.sources.youtube.yt_dlp_path;
    program_versions(&[
        Program {
            name: "ffmpeg",
            path: &config.paths.ffmpeg_path,
            version_flag: "-version",
            origin: format!("paths.ffmpeg_path = {:?}", config.paths.ffmpeg_path),
        },
        Program {
            name: "yt-dlp",
            path: yt_dlp,
            version_flag: "--version",
            origin: format!("sources.youtube.yt_dlp_path = {yt_dlp:?}"),
        },
        Program {
            name: "deno",
            path: Path::new("deno"),
            version_flag: "--version",
            origin: "looked up in PATH, needed by yt-dlp".into(),
        },
    ])
}

/// The first word starting with a digit: `ffmpeg version 7.1 Copyright ...` gives `7.1`,
/// `deno 2.9.8 (stable, ...)` gives `2.9.8`. The whole line if there is none.
fn version_number(line: &str) -> &str {
    line.split_whitespace()
        .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
        .unwrap_or(line.trim())
}

/// Lists every missing program at once, so a new setup is fixed in one go.
fn program_versions(programs: &[Program]) -> anyhow::Result<Vec<(&'static str, String)>> {
    let mut versions = Vec::new();
    let mut missing = Vec::new();
    for program in programs {
        let output = Process::new(program.path)
            .arg(program.version_flag)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        match output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let first_line = stdout.lines().next().unwrap_or_default();
                versions.push((program.name, version_number(first_line).to_owned()));
            }
            Ok(output) => missing.push(format!(
                "{} exited with {} ({})",
                program.name, output.status, program.origin
            )),
            Err(err) => missing.push(format!(
                "{} cannot be started: {err} ({})",
                program.name, program.origin
            )),
        }
    }
    if missing.is_empty() {
        Ok(versions)
    } else {
        anyhow::bail!("missing programs:\n  {}", missing.join("\n  "))
    }
}

async fn run(location: &ConfigLocation, log_level: Option<LogLevel>) -> anyhow::Result<()> {
    let config = location.load()?;
    logging::init(log_level, config.logging.level);
    info!("using config file {}", location.file_path().display());
    for (name, version) in check_dependencies(&config)? {
        info!("{name}: {version}");
    }

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
    // Stops by itself once the client exits and the events channel closes.
    let mut player = tokio::spawn(player.run());

    tokio::select! {
        _ = sigint.recv() => info!("SIGINT received, shutting down"),
        _ = sigterm.recv() => info!("SIGTERM received, shutting down"),
        result = &mut client => return Ok(result??),
        // While the client runs, the player only ends by panicking, which would leave
        // the bot connected but deaf. Exiting with an error lets systemd restart it.
        result = &mut player => {
            result?;
            anyhow::bail!("player stopped while the client was still running");
        }
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

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    use tempfile::TempDir;

    use super::*;

    /// Written by `cp`, not by this process: a test running a program at the same time
    /// would inherit an open write handle and running the script would fail with ETXTBSY.
    fn script(dir: &TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        let source = dir.path().join(format!("{name}.src"));
        fs::write(&source, format!("#!/bin/sh\n{body}\n")).unwrap();
        let copied = Process::new("cp").arg(&source).arg(&path).status().unwrap();
        assert!(copied.success());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn program<'a>(name: &'static str, path: &'a Path) -> Program<'a> {
        Program {
            name,
            path,
            version_flag: "--version",
            origin: format!("{name}_path"),
        }
    }

    #[test]
    fn version_number_is_the_first_word_with_a_digit() {
        let cases = [
            (
                "ffmpeg version 7.1.5-0+deb13u1 Copyright (c) 2000-2026",
                "7.1.5-0+deb13u1",
            ),
            (
                "deno 2.9.8 (stable, release, x86_64-unknown-linux-gnu)",
                "2.9.8",
            ),
            ("2026.10.26", "2026.10.26"),
            ("  no version here ", "no version here"),
            ("", ""),
        ];
        for (line, version) in cases {
            assert_eq!(version_number(line), version, "{line}");
        }
    }

    #[test]
    fn versions_come_from_the_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = script(
            &dir,
            "ffmpeg",
            "echo 'ffmpeg version 7.1 Copyright (c) the FFmpeg developers'; echo 'built with gcc'",
        );
        let yt_dlp = script(&dir, "yt-dlp", r#"[ "$1" = --version ] && echo 2026.10.26"#);
        let versions =
            program_versions(&[program("ffmpeg", &ffmpeg), program("yt-dlp", &yt_dlp)]).unwrap();
        assert_eq!(
            versions,
            [
                ("ffmpeg", "7.1".to_owned()),
                ("yt-dlp", "2026.10.26".to_owned())
            ]
        );
    }

    #[test]
    fn every_missing_program_is_listed_with_its_origin() {
        let dir = tempfile::tempdir().unwrap();
        let ok = script(&dir, "ok", "echo 1.0");
        let failing = script(&dir, "failing", "exit 3");
        let absent = dir.path().join("absent");
        let err = program_versions(&[
            program("ok", &ok),
            program("failing", &failing),
            program("absent", &absent),
        ])
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("failing exited with exit status: 3 (failing_path)"),
            "{err}"
        );
        assert!(err.contains("absent cannot be started"), "{err}");
        assert!(err.contains("(absent_path)"), "{err}");
        assert!(!err.contains("ok "), "{err}");
    }
}
