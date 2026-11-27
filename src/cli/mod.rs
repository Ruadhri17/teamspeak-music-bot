//! Command line interface: the arguments and the commands that only run in a terminal.
//! Doc comments on fields become the `--help` text.
//!
//! - `mod.rs`: `Cli`, `Command` and `init`
//! - `yt_check`: `tsmusic yt-check`
//! - `setup`: `tsmusic setup`, installing the bot as a service, and `tsmusic update`
//!
//! `run` starts the bot itself and stays in `main.rs`.

pub mod setup;
pub mod yt_check;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::{ConfigLocation, LogLevel};

#[derive(Parser, Debug)]
#[command(name = "tsmusic", version, about = "TeamSpeak music bot")]
pub struct Cli {
    /// Path to the config file.
    #[arg(long, short, global = true, default_value = "config.toml")]
    pub config: PathBuf,

    /// Log level. Overrides `RUST_LOG` and `logging.level` from the config.
    #[arg(long, global = true, value_enum)]
    pub log_level: Option<LogLevel>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create config template.
    Init {
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },

    /// Connect to the server and start the bot.
    Run,

    /// Check the config file without starting the bot.
    #[command(name = "check-config")]
    CheckConfig,

    /// Check YouTube playback on this machine, without TeamSpeak.
    #[command(name = "yt-check")]
    YtCheck {
        /// A YouTube link or a search phrase, quotes are optional.
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,

        /// Decode the whole track, not only the first 10 seconds.
        #[arg(long)]
        full: bool,
    },

    /// Install the programs the bot needs and set it up as a systemd service. Run as root.
    Setup {
        /// Take the defaults without asking: install what is missing, set up the service.
        #[arg(long, short)]
        yes: bool,

        /// Download yt-dlp and deno again and restart a running service.
        #[arg(long)]
        update: bool,

        /// Change nothing, only show the commands.
        #[arg(long)]
        dry_run: bool,

        /// Install only the programs, without the service, e.g. on a machine for development.
        #[arg(long)]
        tools_only: bool,
    },

    /// Update the service to the latest release, with its programs. Run as root.
    Update {
        /// Install this release instead of the latest, e.g. 0.3.0.
        #[arg(long)]
        version: Option<String>,

        /// Install the release even when it is the running version.
        #[arg(long)]
        force: bool,
    },
}

/// `tsmusic check-config`: reads and checks the config like `run`, without connecting.
/// `tsmusic update` asks the new version this before installing it.
pub fn check_config(location: &ConfigLocation) -> anyhow::Result<()> {
    location.load()?;
    println!("{} is valid", location.file_path().display());
    Ok(())
}

/// `tsmusic init`: writes the config template.
pub fn init(location: &ConfigLocation, force: bool) -> anyhow::Result<()> {
    location.init(force)?;
    println!("config file created at: {}", location.file_path().display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn config_defaults_to_current_dir() {
        let cli = Cli::parse_from(["tsmusic", "run"]);
        assert_eq!(cli.config, PathBuf::from("config.toml"));
        assert_eq!(cli.log_level, None);
    }

    #[test]
    fn global_flags_work_after_subcommand() {
        let cli = Cli::parse_from([
            "tsmusic",
            "init",
            "--force",
            "-c",
            "/etc/tsmusic/config.toml",
        ]);
        assert_eq!(cli.config, PathBuf::from("/etc/tsmusic/config.toml"));
        assert!(matches!(cli.command, Command::Init { force: true }));
    }

    #[test]
    fn log_level_uses_lowercase_names() {
        let cli = Cli::parse_from(["tsmusic", "--log-level", "trace", "run"]);
        assert_eq!(cli.log_level, Some(LogLevel::Trace));
        assert!(Cli::try_parse_from(["tsmusic", "--log-level", "verbose", "run"]).is_err());
    }

    #[test]
    fn yt_check_joins_the_query_words() {
        let cli = Cli::parse_from(["tsmusic", "yt-check", "daft", "punk", "--full"]);
        let Command::YtCheck { query, full } = cli.command else {
            panic!("expected yt-check");
        };
        assert_eq!(query.join(" "), "daft punk");
        assert!(full);
        assert!(Cli::try_parse_from(["tsmusic", "yt-check"]).is_err());
    }

    #[test]
    fn update_takes_a_version_and_force() {
        let cli = Cli::parse_from(["tsmusic", "update", "--version", "0.3.0", "--force"]);
        let Command::Update { version, force } = cli.command else {
            panic!("expected update");
        };
        assert_eq!(version.as_deref(), Some("0.3.0"));
        assert!(force);
        let cli = Cli::parse_from(["tsmusic", "update"]);
        assert!(matches!(
            cli.command,
            Command::Update {
                version: None,
                force: false
            }
        ));
    }

    #[test]
    fn the_version_of_the_bot_is_still_a_top_level_flag() {
        let err = Cli::try_parse_from(["tsmusic", "--version"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn an_unknown_subcommand_is_a_usage_error() {
        let err = Cli::try_parse_from(["tsmusic", "check-everything"]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(matches!(
            Cli::parse_from(["tsmusic", "check-config"]).command,
            Command::CheckConfig
        ));
    }
}
