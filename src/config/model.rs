//! Config file model: sections, fields and their defaults.
//!
//! Missing fields fall back to `Default`, unknown keys are rejected.
//! Defaults must match `config.toml.template`.

use std::{num::NonZeroUsize, path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};

use super::values::{Bitrate, Complexity, Volume};

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
    /// bot nickname
    pub nickname: String,
    /// bot description
    pub description: String,
    /// Relative to `paths.state_dir`.
    pub identity_file: PathBuf,
    /// Set `None` for server's default channel.
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
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
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
        Self {
            bitrate: Bitrate::new(96).unwrap(),
            complexity: Complexity::new(10).unwrap(),
            start_buffer: Duration::from_secs(3),
            max_buffer: Duration::from_secs(5),
            default_volume: Volume::new(50).unwrap(),
            max_volume: Volume::new(100).unwrap(),
            pause_release_after: Duration::from_secs(5 * 60),
        }
    }
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct EventsConfig {
    pub on_connect: ConnectEvent,
    pub on_disconnect: DisconnectEvent,
    pub on_idle: IdleEvent,
    pub on_alone: AloneEvent,
    pub on_party: PartyEvent,
    pub on_song_start: SongStartEvent,
}

/// Bot connected to the server.
#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ConnectEvent {
    pub message: Option<String>,
}

/// Bot disconnects on shutdown.
#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DisconnectEvent {
    pub message: Option<String>,
}

/// Nothing played for `delay` (pause counts as idle).
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct IdleEvent {
    #[serde(with = "humantime_serde")]
    pub delay: Duration,
    pub sound: Option<PathBuf>,
    pub message: Option<String>,
    pub return_home: bool,
}

impl Default for IdleEvent {
    fn default() -> Self {
        Self {
            delay: Duration::from_secs(10 * 60),
            sound: None,
            message: None,
            return_home: true,
        }
    }
}

/// Bot alone in its channel for `delay`.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AloneEvent {
    #[serde(with = "humantime_serde")]
    pub delay: Duration,
    pub sound: Option<PathBuf>,
    pub message: Option<String>,
    pub return_home: bool,
}

impl Default for AloneEvent {
    fn default() -> Self {
        Self {
            delay: Duration::from_secs(60),
            sound: None,
            message: None,
            return_home: true,
        }
    }
}

/// Bot was alone and someone joined (or bot entered a channel with users).
#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PartyEvent {
    #[serde(with = "humantime_serde")]
    pub delay: Duration,
    /// Played before the queue.
    pub sound: Option<PathBuf>,
    pub message: Option<String>,
}

/// A new song started.
#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SongStartEvent {
    /// Placeholders: `{title}`, `{artist}`, `{source}`.
    pub message: Option<String>,
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
            show_page_size: NonZeroUsize::new(15).unwrap(),
        }
    }
}

/// Checked in order: admin -> blocked -> user.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionsConfig {
    pub admin_uids: Vec<String>,
    pub admin_groups: Vec<GroupRef>,
    /// Empty `user_*` = everyone is a user.
    pub user_uids: Vec<String>,
    pub user_groups: Vec<GroupRef>,
    pub blocked_uids: Vec<String>,
    pub blocked_groups: Vec<GroupRef>,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            admin_uids: Vec::new(),
            admin_groups: vec![
                GroupRef::Name("Admin".into()),
                GroupRef::Name("Owner".into()),
            ],
            user_uids: Vec::new(),
            user_groups: Vec::new(),
            blocked_uids: Vec::new(),
            blocked_groups: Vec::new(),
        }
    }
}

/// Server group ID or name (case-insensitive).
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum GroupRef {
    Id(u64),
    Name(String),
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SourcesConfig {
    /// Source used for search phrases.
    pub default: SourceKind,
    pub tidal: TidalConfig,
    pub youtube: YoutubeConfig,
    pub local: LocalConfig,
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    #[default]
    Tidal,
    Spotify,
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
}

impl Default for YoutubeConfig {
    fn default() -> Self {
        Self {
            yt_dlp_path: "yt-dlp".into(),
        }
    }
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LocalConfig {
    /// Set to `None` for local source disabled.
    pub music_dir: Option<PathBuf>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PathsConfig {
    /// Identity and Tidal session.
    pub state_dir: PathBuf,
    pub ffmpeg_path: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            state_dir: "state".into(),
            ffmpeg_path: "ffmpeg".into(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingConfig {
    /// Overridden by `RUST_LOG` and the CLI flag.
    pub level: LogLevel,
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

#[derive(Deserialize, Serialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}
