//! Chat command parser: `!song`, `!queue`, `!bot`.
//!
//! Turns message text into a `Command` with flags (`--source`, `--track|--album|--playlist`)
//! and returns readable usage errors. Does not execute commands or check permissions.

mod error;

pub use error::CommandError;

use std::num::NonZeroUsize;

use crate::{config::SourceKind, sources::SearchKind};

pub type CommandResult = Result<Command, CommandError>;

const SONG_COMMANDS: &str = "!song add|pause|resume|next|current";
const QUEUE_COMMANDS: &str = "!queue show|remove|clear";
const BOT_COMMANDS: &str = "!bot summon|config";
const CONFIG_COMMANDS: &str = "!bot config list|get|set|reload";

const ADD_USAGE: &str = "!song add [--track|--album|--playlist] [--source <name>] <link | phrase>";
const SHOW_USAGE: &str = "!queue show [page]";
const REMOVE_USAGE: &str = "!queue remove <number>";
const LIST_USAGE: &str = "!bot config list [section]";
const GET_USAGE: &str = "!bot config get <section.key>";
const SET_USAGE: &str = "!bot config set <section.key> <value>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `!bot ping`, a liveness check.
    Ping,
    /// `!song add [flags] <link | phrase>`.
    Add {
        /// A link or a search phrase, `sources` decides which.
        query: String,
        /// `None` means `sources.default` from the config.
        source: Option<SourceKind>,
        /// Ignored for links, they carry their own type.
        kind: SearchKind,
    },
    /// `!song pause`, `!song stop`.
    Pause,
    /// `!song resume`.
    Resume,
    /// `!song next`, `!song skip`.
    Next,
    /// `!song current`.
    Current,
    /// `!queue show [page]`, `!queue display [page]`. Pages count from 1.
    QueueShow { page: NonZeroUsize },
    /// `!queue remove <number>`, the number as shown by `!queue show`.
    QueueRemove { position: NonZeroUsize },
    /// `!queue clear`.
    QueueClear,
    /// `!bot summon`.
    Summon,
    /// `!bot config list [section]`.
    ConfigList { section: Option<String> },
    /// `!bot config get <section.key>`.
    ConfigGet { key: String },
    /// `!bot config set <section.key> <value>`, the value may contain spaces.
    ConfigSet { key: String, value: String },
    /// `!bot config reload`.
    ConfigReload,
}

impl Command {
    /// `None`: not a command at all (normal chat), the bot stays silent.
    /// `Some(Err)`: a known prefix with a wrong command, the bot answers with the error.
    pub fn parse(text: &str) -> Option<CommandResult> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let result = match words.as_slice() {
            ["!song", rest @ ..] => parse_song(rest),
            ["!queue", rest @ ..] => parse_queue(rest),
            ["!bot", rest @ ..] => parse_bot(rest),
            _ => return None,
        };
        Some(result)
    }
}

fn parse_song(words: &[&str]) -> CommandResult {
    match words {
        ["add", rest @ ..] => parse_add(rest),
        ["pause" | "stop"] => Ok(Command::Pause),
        ["resume"] => Ok(Command::Resume),
        ["next" | "skip"] => Ok(Command::Next),
        ["current"] => Ok(Command::Current),
        _ => Err(CommandError::UnknownCommand(SONG_COMMANDS)),
    }
}

/// Flags come before the phrase, in any order. The phrase starts at the first word
/// that is not a flag.
fn parse_add(mut words: &[&str]) -> CommandResult {
    let mut source = None;
    let mut kind = None;

    loop {
        match words {
            ["--source", name, tail @ ..] => {
                let parsed = source_kind(name)
                    .ok_or_else(|| CommandError::UnknownSource(name.to_string()))?;
                if source.replace(parsed).is_some() {
                    return Err(CommandError::RepeatedFlag("--source"));
                }
                words = tail;
            }
            ["--source"] => return Err(CommandError::MissingValue("--source")),
            // Any other `--` word must be a kind flag, so a typo is an error, not part of the phrase.
            [flag, tail @ ..] if flag.starts_with("--") => {
                let parsed =
                    search_kind(flag).ok_or_else(|| CommandError::UnknownFlag(flag.to_string()))?;
                if kind.replace(parsed).is_some() {
                    return Err(CommandError::ConflictingKinds);
                }
                words = tail;
            }
            _ => break,
        }
    }

    if words.is_empty() {
        return Err(CommandError::Usage(ADD_USAGE));
    }
    Ok(Command::Add {
        query: words.join(" "),
        source,
        kind: kind.unwrap_or_default(),
    })
}

fn parse_queue(words: &[&str]) -> CommandResult {
    match words {
        ["show" | "display"] => Ok(Command::QueueShow {
            page: NonZeroUsize::MIN,
        }),
        ["show" | "display", page] => Ok(Command::QueueShow {
            page: number(page)?,
        }),
        ["show" | "display", ..] => Err(CommandError::Usage(SHOW_USAGE)),
        ["remove", position] => Ok(Command::QueueRemove {
            position: number(position)?,
        }),
        ["remove", ..] => Err(CommandError::Usage(REMOVE_USAGE)),
        ["clear"] => Ok(Command::QueueClear),
        _ => Err(CommandError::UnknownCommand(QUEUE_COMMANDS)),
    }
}

fn parse_bot(words: &[&str]) -> CommandResult {
    match words {
        ["ping"] => Ok(Command::Ping),
        ["summon"] => Ok(Command::Summon),
        ["config", rest @ ..] => parse_config(rest),
        _ => Err(CommandError::UnknownCommand(BOT_COMMANDS)),
    }
}

fn parse_config(words: &[&str]) -> CommandResult {
    match words {
        ["list"] => Ok(Command::ConfigList { section: None }),
        ["list", section] => Ok(Command::ConfigList {
            section: Some(section.to_string()),
        }),
        ["list", ..] => Err(CommandError::Usage(LIST_USAGE)),
        ["get", key] => Ok(Command::ConfigGet {
            key: key.to_string(),
        }),
        ["get", ..] => Err(CommandError::Usage(GET_USAGE)),
        ["set", key, value @ ..] if !value.is_empty() => Ok(Command::ConfigSet {
            key: key.to_string(),
            value: value.join(" "),
        }),
        ["set", ..] => Err(CommandError::Usage(SET_USAGE)),
        ["reload"] => Ok(Command::ConfigReload),
        _ => Err(CommandError::UnknownCommand(CONFIG_COMMANDS)),
    }
}

/// Case insensitive.
fn source_kind(name: &str) -> Option<SourceKind> {
    match name.to_ascii_lowercase().as_str() {
        "tidal" => Some(SourceKind::Tidal),
        "spotify" => Some(SourceKind::Spotify),
        "youtube" => Some(SourceKind::Youtube),
        "soundcloud" => Some(SourceKind::Soundcloud),
        "local" => Some(SourceKind::Local),
        _ => None,
    }
}

fn search_kind(flag: &str) -> Option<SearchKind> {
    match flag {
        "--track" => Some(SearchKind::Track),
        "--album" => Some(SearchKind::Album),
        "--playlist" => Some(SearchKind::Playlist),
        _ => None,
    }
}

/// A page or position, counted from 1.
fn number(text: &str) -> Result<NonZeroUsize, CommandError> {
    text.parse()
        .map_err(|_| CommandError::InvalidNumber(text.to_string()))
}

#[cfg(test)]
mod tests;
