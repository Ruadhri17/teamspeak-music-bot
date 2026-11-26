//! Config file model: sections, fields and their defaults.
//!
//! Missing fields fall back to `Default`, unknown keys are rejected.
//! Defaults must match `config.example.toml`.

use std::{num::NonZeroUsize, path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use url::Url;

use super::values::{Bitrate, Complexity, Volume};
use crate::{
    sources::{QueryType, parse_query},
    template::Template,
};

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub teamspeak: TeamspeakConfig,
    pub audio: AudioConfig,
    pub events: EventsConfig,
    pub queue: QueueConfig,
    pub permissions: PermissionsConfig,
    pub sources: SourcesConfig,
    pub paths: PathsConfig,
    pub logging: LoggingConfig,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TeamspeakConfig {
    /// Host, `host:port`, IP or server nickname, resolved by tsclientlib.
    pub address: String,
    pub nickname: String,
    pub description: String,
    /// Relative to `paths.state_dir`.
    pub identity_file: PathBuf,
    /// `None` uses the server's default channel.
    pub home_channel: Option<ChannelRef>,
}

impl Default for TeamspeakConfig {
    fn default() -> Self {
        Self {
            address: "localhost".into(),
            nickname: "MusicBot".into(),
            description: "Exemplary description".into(),
            identity_file: "identity.toml".into(),
            home_channel: None,
        }
    }
}

/// Channel ID or name/path ("Parent/Channel").
#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq)]
#[serde(untagged)]
pub enum ChannelRef {
    Id(u64),
    Name(String),
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    pub bitrate: Bitrate,
    pub complexity: Complexity,
    /// Gathered before the bot starts speaking, from silence or after the buffer ran dry.
    #[serde(with = "humantime_serde")]
    pub start_buffer: Duration,
    #[serde(with = "humantime_serde")]
    pub max_buffer: Duration,
    pub default_volume: Volume,
    pub max_volume: Volume,
    /// Release the stream after being paused this long.
    #[serde(with = "humantime_serde")]
    pub pause_release_after: Duration,
}

impl Default for AudioConfig {
    fn default() -> Self {
        // Constants within the limits of each type, so `unwrap` cannot fail.
        Self {
            bitrate: Bitrate::new(96).unwrap(),
            complexity: Complexity::new(10).unwrap(),
            start_buffer: Duration::from_secs(3),
            max_buffer: Duration::from_secs(15),
            default_volume: Volume::new(50).unwrap(),
            max_volume: Volume::new(100).unwrap(),
            pause_release_after: Duration::from_secs(5 * 60),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct EventsConfig {
    /// The bot connected to the server, `delay` after the channel list arrived. Also after
    /// a reconnect, the server forgets the description then.
    pub on_connect: Event,
    pub on_disconnect: DisconnectEvent,
    /// Nothing played for `delay` away from home (pause counts as idle).
    pub on_idle: Event,
    /// The bot alone in its channel for `delay`, away from home.
    pub on_alone: Event,
    /// Someone is in the bot's channel and nothing plays, after `delay`.
    pub on_party: Event,
    /// A song from the queue started. Its texts may use `{song}` and `{source}`. It cannot
    /// have a `sound`, the song already plays.
    pub on_song_start: Event,
}

/// Used for a missing `[events.*]` section. A written section has only the actions it lists.
impl Default for EventsConfig {
    fn default() -> Self {
        let back_home = |delay| Event {
            delay,
            move_to: Some(Destination::Home),
            ..Event::default()
        };
        Self {
            on_connect: Event::default(),
            on_disconnect: DisconnectEvent::default(),
            on_idle: back_home(Duration::from_secs(10 * 60)),
            on_alone: back_home(Duration::from_secs(60)),
            on_party: Event::default(),
            on_song_start: Event::default(),
        }
    }
}

/// Bot disconnects on shutdown. After that nothing can be done, so it has no actions,
/// only the reason users see.
#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DisconnectEvent {
    pub message: Option<String>,
}

/// What an event does, `delay` after its condition is met. Every action is optional, they
/// run in this order: `message`, `sound` (to its end), then `move_to`, `description` and
/// `nickname`. Validation rejects a zero `delay` where it would make the bot leave at once.
/// `description` and `nickname` may use `{default}`, the value from `[teamspeak]`.
#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Event {
    #[serde(with = "humantime_serde")]
    pub delay: Duration,
    /// Channel message, sent as written (BBCode included).
    pub message: Option<Template>,
    /// Played outside the queue to its end, the remaining actions wait for it.
    pub sound: Option<Sound>,
    pub move_to: Option<Destination>,
    pub description: Option<Template>,
    pub nickname: Option<Template>,
}

/// The `sound` of an event: a file, or a link the bot looks up once after starting.
/// Written as one string, a link is told from a path the same way `!song add` does it.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(from = "String", into = "String")]
pub enum Sound {
    /// Relative to the config folder or absolute, anywhere on the disk: the config is
    /// written by the admin, not by people on the chat.
    File(PathBuf),
    Link(Url),
}

impl From<String> for Sound {
    fn from(text: String) -> Self {
        match parse_query(text.trim()) {
            QueryType::Link(url) => Sound::Link(url),
            QueryType::Phrase(_) => Sound::File(text.into()),
        }
    }
}

impl From<Sound> for String {
    fn from(sound: Sound) -> Self {
        match sound {
            Sound::File(path) => path.to_string_lossy().into_owned(),
            Sound::Link(url) => url.into(),
        }
    }
}

/// Where an event moves the bot: `move_to = "home"` or `move_to = { channel = 22 }`.
#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Destination {
    Home,
    Channel(ChannelRef),
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct QueueConfig {
    /// `None` = unlimited.
    pub max_len: Option<NonZeroUsize>,
    /// Entries per page in `!queue show`.
    pub show_page_size: NonZeroUsize,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            max_len: None,
            // Not zero, so `unwrap` cannot fail.
            show_page_size: NonZeroUsize::new(15).unwrap(),
        }
    }
}

/// Roles come from server groups. Everyone else in the bot's channel is a user.
/// Dragging the bot and joining channels are left to the server permissions.
/// Both lists are empty by default, group names differ from server to server.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionsConfig {
    /// May use `!bot config` and summon the bot while it is busy elsewhere.
    pub admin_groups: Vec<GroupRef>,
    /// May not control or summon the bot. Admin takes precedence.
    pub blocked_groups: Vec<GroupRef>,
}

/// Server group ID or name (case-insensitive).
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum GroupRef {
    Id(u64),
    Name(String),
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SourcesConfig {
    /// Source used for search phrases.
    pub default: SourceKind,
    /// The most tracks one playlist link adds, the rest of the playlist is left out.
    pub max_playlist_len: NonZeroUsize,
    /// How many `!song add` lookups run at the same time, the rest wait their turn.
    /// Each one is a `yt-dlp` process, so this bounds the memory a burst of commands takes.
    pub max_parallel_lookups: NonZeroUsize,
    pub tidal: TidalConfig,
    pub youtube: YoutubeConfig,
    pub local: LocalConfig,
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            default: SourceKind::default(),
            max_playlist_len: NonZeroUsize::new(500).unwrap(),
            max_parallel_lookups: NonZeroUsize::new(2).unwrap(),
            tidal: TidalConfig::default(),
            youtube: YoutubeConfig::default(),
            local: LocalConfig::default(),
        }
    }
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Tidal,
    Spotify,
    #[default]
    Youtube,
    Soundcloud,
    Local,
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TidalConfig {
    pub quality: TidalQuality,
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TidalQuality {
    Low,
    #[default]
    High,
    Lossless,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct YoutubeConfig {
    pub yt_dlp_path: PathBuf,
    /// YouTube clients tried first. If they fail, `yt-dlp` retries with its own defaults.
    /// Empty = `yt-dlp` always chooses.
    pub player_clients: Vec<String>,
}

impl Default for YoutubeConfig {
    fn default() -> Self {
        Self {
            yt_dlp_path: "yt-dlp".into(),
            // Needs no JS challenges, so audio starts in about 2 s instead of about 9 s.
            player_clients: vec!["visionos".into()],
        }
    }
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LocalConfig {
    /// `None` disables the local source.
    pub music_dir: Option<PathBuf>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PathsConfig {
    /// Identity and Tidal session.
    pub state_dir: PathBuf,
    /// Passwords and API keys, must be `0600`. Ignored under systemd credentials.
    pub secrets_file: PathBuf,
    pub ffmpeg_path: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            state_dir: "state".into(),
            secrets_file: "secrets.toml".into(),
            ffmpeg_path: "ffmpeg".into(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Overridden by `RUST_LOG` and the CLI flag.
    pub level: LogLevel,
    /// How often the client logs its audio statistics, see `teamspeak::metrics`.
    #[serde(with = "humantime_serde")]
    pub metrics_interval: Duration,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            metrics_interval: Duration::from_secs(60),
        }
    }
}

/// `ValueEnum` lets `--log-level` use the same lowercase names as the config file.
#[derive(Deserialize, Serialize, Default, Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}
