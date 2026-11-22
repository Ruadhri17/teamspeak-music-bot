//! Command line interface. Doc comments on fields become the `--help` text.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::LogLevel;

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
}
