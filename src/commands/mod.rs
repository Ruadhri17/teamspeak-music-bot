//! Chat command parser: `!song`, `!queue`, `!bot`, `!help`.
//!
//! Turns message text into a `Command` with flags (`--source`, `--track|--album|--playlist`)
//! and returns readable usage errors. Does not execute commands or check permissions.

mod error;

pub use error::CommandError;

use std::num::NonZeroUsize;

use crate::{
    config::{SourceKind, Volume},
    sources::{QueryType, SearchKind, parse_query},
};

pub type CommandResult = Result<Command, CommandError>;

const PREFIXES: [&str; 4] = ["!song", "!queue", "!bot", "!help"];

const SONG_COMMANDS: &str = "!song add|pause|resume|next|current|help";
const QUEUE_COMMANDS: &str = "!queue show|remove|clear|help";
const BOT_COMMANDS: &str = "!bot summon|volume|ping|config|help";
const CONFIG_COMMANDS: &str = "!bot config list|get|set|reload|help";

const ADD_USAGE: &str = "!song add [--track|--album|--playlist] [--source <name>] <link | phrase>";
const VOLUME_USAGE: &str = "!bot volume [0-100]";
const SHOW_USAGE: &str = "!queue show|display [page]";
const REMOVE_USAGE: &str = "!queue remove <number>";
const LIST_USAGE: &str = "!bot config list [section]";
const GET_USAGE: &str = "!bot config get <section.key>";
const SET_USAGE: &str = "!bot config set <section.key> <value>";

/// `!help`, the starting point: one line per prefix, each points to its own help.
const GENERAL_HELP: &str = "\
Available commands:
!song help - adding songs and controlling playback
!queue help - showing and editing the queue
!bot help - summoning, volume and settings of the bot";

// Help texts repeat the `*_USAGE` lines, a test checks that they stay the same.
// One line per command, well under the 1024 character limit of a TeamSpeak message.
const SONG_HELP: &str = "\
Available commands:
!song add [--track|--album|--playlist] [--source <name>] <link | phrase> - add to the queue
!song pause|stop - pause the current song
!song resume - continue playing
!song next|skip - skip to the next song
!song current - show what is playing
!song add --help - more about adding";

const ADD_HELP: &str = "\
usage: !song add [--track|--album|--playlist] [--source <name>] <link | phrase>
A link plays what it points to, a phrase is searched.
--source tidal|spotify|youtube|soundcloud|local - where to search, default from the config
--track|--album|--playlist - what a phrase looks for, default --track";

const QUEUE_HELP: &str = "\
Available commands:
!queue show|display [page] - list the queue
!queue remove <number> - remove a song, the number as in !queue show
!queue clear - remove everything";

const BOT_HELP: &str = "\
Available commands:
!bot summon - move the bot to your channel
!bot volume [0-100] - show or set the volume for all songs
!bot ping - check that the bot answers
!bot config list|get|set|reload - settings, see !bot config help";

const CONFIG_HELP: &str = "\
Available commands:
!bot config list [section] - show the settings
!bot config get <section.key> - show one setting
!bot config set <section.key> <value> - change a setting
!bot config reload - read the config file again";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `!bot ping`, a liveness check.
    Ping,
    /// `!help`, a bare prefix, or `help` / `--help` after it. The text is the chat reply.
    Help(&'static str),
    /// `!song add [flags] <link | phrase>`.
    Add {
        /// A link or a search phrase, `sources` decides which.
        query: QueryType,
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
    /// `!bot volume [0-100]`, `None` shows the current volume. Applies to all songs.
    Volume(Option<Volume>),
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
            // Extra words are ignored, `!help me` should still help.
            ["!help", ..] => Ok(Command::Help(GENERAL_HELP)),
            [first, ..] => {
                return suggest(first).map(|prefix| Err(CommandError::DidYouMean(prefix)));
            }
            [] => return None,
        };
        Some(result)
    }
}

/// A known prefix at most one typo away from `word`, so a mistyped command gets an answer.
/// Commands of other bots in the channel (`!play`, `!stop`) are further away and stay ignored.
fn suggest(word: &str) -> Option<&'static str> {
    if !word.starts_with('!') {
        return None;
    }
    // Lowercase, so `!Song` is answered too.
    let word = word.to_lowercase();
    PREFIXES
        .into_iter()
        .find(|prefix| typo_distance(&word, prefix) <= 1)
}

/// Edits needed to turn `a` into `b`: insert, delete or replace a character, or swap two
/// neighbours (`!snog`), each counts as 1. Counted in characters, not bytes.
fn typo_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    // Only three rows of the table are needed: two back for swaps, the previous and the current.
    let mut two_back = vec![0; b.len() + 1];
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut current = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let replace = usize::from(a[i - 1] != b[j - 1]);
            current[j] = (previous[j] + 1)
                .min(current[j - 1] + 1)
                .min(previous[j - 1] + replace);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                current[j] = current[j].min(two_back[j - 2] + 1);
            }
        }
        two_back = std::mem::replace(&mut previous, current);
    }
    previous[b.len()]
}

fn parse_song(words: &[&str]) -> CommandResult {
    match words {
        [] | ["help" | "--help"] => Ok(Command::Help(SONG_HELP)),
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
            // Only before the phrase, so `!song add help` still searches for "help".
            ["--help", ..] => return Ok(Command::Help(ADD_HELP)),
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

    let text = words.join(" ");
    let text = strip_bbcode(&text).trim();
    if text.is_empty() {
        return Err(CommandError::Usage(ADD_USAGE));
    }
    Ok(Command::Add {
        query: parse_query(text),
        source,
        kind: kind.unwrap_or_default(),
    })
}

/// Removes the `[URL]` tag that TeamSpeak 3 puts around a link filling the whole text.
/// `[URL=link]text[/URL]` gives the link, not the text shown in the chat.
fn strip_bbcode(text: &str) -> &str {
    let Some(inner) = strip_suffix_ignore_case(text, "[/url]") else {
        return text;
    };
    if let Some(link) = strip_prefix_ignore_case(inner, "[url]") {
        return link;
    }
    if let Some((link, _)) =
        strip_prefix_ignore_case(inner, "[url=").and_then(|rest| rest.split_once(']'))
    {
        return link;
    }
    text
}

/// `get` returns `None` instead of panicking when the cut falls inside a multi-byte character.
fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let start = text.len().checked_sub(suffix.len())?;
    let tail = text.get(start..)?;
    tail.eq_ignore_ascii_case(suffix).then(|| &text[..start])
}

fn parse_queue(words: &[&str]) -> CommandResult {
    match words {
        [] | ["help" | "--help"] => Ok(Command::Help(QUEUE_HELP)),
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
        [] | ["help" | "--help"] => Ok(Command::Help(BOT_HELP)),
        ["ping"] => Ok(Command::Ping),
        ["summon"] => Ok(Command::Summon),
        ["volume"] => Ok(Command::Volume(None)),
        ["volume", percent] => volume(percent).map(|volume| Command::Volume(Some(volume))),
        ["config", rest @ ..] => parse_config(rest),
        _ => Err(CommandError::UnknownCommand(BOT_COMMANDS)),
    }
}

fn parse_config(words: &[&str]) -> CommandResult {
    match words {
        [] | ["help" | "--help"] => Ok(Command::Help(CONFIG_HELP)),
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

/// 0-100, a `%` at the end is allowed. Anything else answers with the usage.
fn volume(text: &str) -> Result<Volume, CommandError> {
    text.strip_suffix('%')
        .unwrap_or(text)
        .parse()
        .ok()
        .and_then(Volume::new)
        .ok_or(CommandError::Usage(VOLUME_USAGE))
}

/// A page or position, counted from 1.
fn number(text: &str) -> Result<NonZeroUsize, CommandError> {
    text.parse()
        .map_err(|_| CommandError::InvalidNumber(text.to_string()))
}

#[cfg(test)]
mod tests;
