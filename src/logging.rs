//! Logging with `tracing`: written to stderr, colors only when stderr is a terminal.
//!
//! Level priority: `--log-level`, then `RUST_LOG`, then `logging.level` from the config.

use std::io::IsTerminal;

use tracing::{level_filters::LevelFilter, warn};
use tracing_subscriber::{
    EnvFilter, Registry, fmt, layer::SubscriberExt, reload, util::SubscriberInitExt,
};

use crate::config::LogLevel;

/// Remote control for the log filter. Cheap to clone, can be sent to other tasks.
pub type LogHandle = reload::Handle<EnvFilter, Registry>;

/// Libraries that log every packet at `info` and below. Kept at `warn` unless `RUST_LOG` says
/// otherwise, so `--log-level trace` shows bot's code, not TeamSpeak protocol internals.
const QUIET_CRATES: [&str; 2] = ["tsclientlib", "tsproto"];

impl From<LogLevel> for LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => LevelFilter::ERROR,
            LogLevel::Warn => LevelFilter::WARN,
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Debug => LevelFilter::DEBUG,
            LogLevel::Trace => LevelFilter::TRACE,
        }
    }
}

/// Call once in `run`, after the config is loaded. Errors before that (bad config file)
/// are printed by `main` through anyhow, not through tracing.
pub fn init(cli_level: Option<LogLevel>, config_level: LogLevel) -> LogHandle {
    let rust_log = std::env::var("RUST_LOG").ok();
    let filter = build_filter(cli_level, rust_log.as_deref(), config_level);

    let (filter, handle) = reload::Layer::new(filter);
    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(std::io::stderr().is_terminal()),
        )
        .init();

    // Reported only now, because before `init` there is nowhere to log it.
    if cli_level.is_none()
        && let Some(value) = rust_log.as_deref().filter(|value| is_invalid(value))
    {
        warn!("ignoring invalid RUST_LOG={value:?}, using logging.level from the config");
    }

    handle
}

/// Picks the filter by priority. `rust_log` is the value of `RUST_LOG`, passed in so tests
/// do not have to change the environment. Empty or invalid `RUST_LOG` counts as unset.
pub fn build_filter(
    cli_level: Option<LogLevel>,
    rust_log: Option<&str>,
    config_level: LogLevel,
) -> EnvFilter {
    if let Some(level) = cli_level {
        return level_filter(level);
    }
    if let Some(filter) = rust_log
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| EnvFilter::try_new(value).ok())
    {
        return filter;
    }
    level_filter(config_level)
}

/// `level` for everything, but no more than `warn` for `QUIET_CRATES`.
pub fn level_filter(level: LogLevel) -> EnvFilter {
    let level = LevelFilter::from(level);
    let quiet = level.min(LevelFilter::WARN);
    QUIET_CRATES.iter().fold(
        EnvFilter::default().add_directive(level.into()),
        |filter, name| {
            let directive = format!("{name}={quiet}")
                .parse()
                .expect("crate names in QUIET_CRATES are valid directives");
            filter.add_directive(directive)
        },
    )
}

fn is_invalid(rust_log: &str) -> bool {
    !rust_log.trim().is_empty() && EnvFilter::try_new(rust_log).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(cli: Option<LogLevel>, rust_log: Option<&str>, config: LogLevel) -> String {
        build_filter(cli, rust_log, config).to_string()
    }

    #[test]
    fn log_level_maps_to_level_filter() {
        assert_eq!(LevelFilter::from(LogLevel::Error), LevelFilter::ERROR);
        assert_eq!(LevelFilter::from(LogLevel::Warn), LevelFilter::WARN);
        assert_eq!(LevelFilter::from(LogLevel::Info), LevelFilter::INFO);
        assert_eq!(LevelFilter::from(LogLevel::Debug), LevelFilter::DEBUG);
        assert_eq!(LevelFilter::from(LogLevel::Trace), LevelFilter::TRACE);
    }

    #[test]
    fn cli_level_wins_over_rust_log_and_config() {
        assert_eq!(
            filter(
                Some(LogLevel::Debug),
                Some("tsmusic=trace"),
                LogLevel::Error
            ),
            level_filter(LogLevel::Debug).to_string()
        );
    }

    #[test]
    fn rust_log_wins_over_config() {
        assert_eq!(
            filter(None, Some("tsmusic=trace"), LogLevel::Error),
            "tsmusic=trace"
        );
    }

    #[test]
    fn config_level_is_used_when_nothing_else_is_set() {
        assert_eq!(
            filter(None, None, LogLevel::Warn),
            level_filter(LogLevel::Warn).to_string()
        );
    }

    #[test]
    fn empty_or_invalid_rust_log_falls_back_to_config() {
        let config = level_filter(LogLevel::Info).to_string();
        for rust_log in ["", "   ", "tsmusic=loud"] {
            assert_eq!(
                filter(None, Some(rust_log), LogLevel::Info),
                config,
                "{rust_log:?}"
            );
        }
    }

    #[test]
    fn level_filter_keeps_teamspeak_crates_at_warn() {
        let filter = level_filter(LogLevel::Trace).to_string();
        assert!(filter.contains("tsclientlib=warn"), "{filter}");
        assert!(filter.contains("tsproto=warn"), "{filter}");
        assert!(filter.contains("trace"), "{filter}");
    }

    #[test]
    fn level_filter_never_makes_teamspeak_crates_louder() {
        let filter = level_filter(LogLevel::Error).to_string();
        assert!(filter.contains("tsclientlib=error"), "{filter}");
        assert!(filter.contains("tsproto=error"), "{filter}");
    }

    #[test]
    fn invalid_rust_log_detection() {
        assert!(is_invalid("tsmusic=loud"));
        assert!(!is_invalid("tsmusic=trace,info"));
        assert!(!is_invalid(""));
    }
}
