use std::{ops::RangeInclusive, path::Path};

use humantime::format_duration;

use super::{AudioConfig, ChannelRef, Config, EventsConfig, TeamspeakConfig};

/// TeamSpeak refuses nicknames outside this length (in characters).
const NICKNAME_LEN: RangeInclusive<usize> = 3..=30;

const EMPTY: &str = "must not be empty";
const EMPTY_OPTIONAL: &str = "must not be empty, remove the line to turn it off";

#[derive(Debug, thiserror::Error)]
#[error("{field}: {reason}")]
pub struct ValidationError {
    pub field: &'static str,
    pub reason: String,
}

/// Returns the first broken rule.
pub fn validate(config: &Config) -> Result<(), ValidationError> {
    teamspeak(&config.teamspeak)?;
    audio(&config.audio)?;
    events(&config.events)?;

    not_empty_path("paths.state_dir", &config.paths.state_dir)?;
    not_empty_path("paths.ffmpeg_path", &config.paths.ffmpeg_path)?;
    not_empty_path(
        "sources.youtube.yt_dlp_path",
        &config.sources.youtube.yt_dlp_path,
    )?;
    optional_path(
        "sources.local.music_dir",
        config.sources.local.music_dir.as_deref(),
    )?;

    // `tokio::time::interval` panics on a zero period.
    ensure(
        !config.logging.metrics_interval.is_zero(),
        "logging.metrics_interval",
        || "must be greater than 0".into(),
    )
}

fn teamspeak(ts: &TeamspeakConfig) -> Result<(), ValidationError> {
    not_blank("teamspeak.address", &ts.address)?;
    not_blank("teamspeak.nickname", &ts.nickname)?;

    let len = ts.nickname.chars().count();
    ensure(NICKNAME_LEN.contains(&len), "teamspeak.nickname", || {
        format!(
            "must be {} to {} characters long (has {len})",
            NICKNAME_LEN.start(),
            NICKNAME_LEN.end()
        )
    })?;

    not_empty_path("teamspeak.identity_file", &ts.identity_file)?;

    if let Some(ChannelRef::Name(name)) = &ts.home_channel {
        not_blank("teamspeak.home_channel", name)?;
    }
    Ok(())
}

fn audio(audio: &AudioConfig) -> Result<(), ValidationError> {
    ensure(
        audio.default_volume <= audio.max_volume,
        "audio.default_volume",
        || {
            format!(
                "must not be greater than audio.max_volume ({} > {})",
                audio.default_volume.get(),
                audio.max_volume.get()
            )
        },
    )?;

    ensure(!audio.max_buffer.is_zero(), "audio.max_buffer", || {
        "must be greater than 0".into()
    })?;

    ensure(
        audio.start_buffer <= audio.max_buffer,
        "audio.start_buffer",
        || {
            format!(
                "must not be greater than audio.max_buffer ({} > {})",
                format_duration(audio.start_buffer),
                format_duration(audio.max_buffer)
            )
        },
    )
}

fn events(events: &EventsConfig) -> Result<(), ValidationError> {
    // A zero delay would make the bot leave as soon as playback stops or the channel empties.
    ensure(
        !events.on_idle.delay.is_zero(),
        "events.on_idle.delay",
        || "must be greater than 0".into(),
    )?;
    ensure(
        !events.on_alone.delay.is_zero(),
        "events.on_alone.delay",
        || "must be greater than 0".into(),
    )?;

    optional_path("events.on_idle.sound", events.on_idle.sound.as_deref())?;
    optional_path("events.on_alone.sound", events.on_alone.sound.as_deref())?;
    optional_path("events.on_party.sound", events.on_party.sound.as_deref())?;

    optional_text(
        "events.on_connect.message",
        events.on_connect.message.as_deref(),
    )?;
    optional_text(
        "events.on_disconnect.message",
        events.on_disconnect.message.as_deref(),
    )?;
    optional_text("events.on_idle.message", events.on_idle.message.as_deref())?;
    optional_text(
        "events.on_alone.message",
        events.on_alone.message.as_deref(),
    )?;
    optional_text(
        "events.on_party.message",
        events.on_party.message.as_deref(),
    )?;
    optional_text(
        "events.on_song_start.message",
        events.on_song_start.message.as_deref(),
    )
}

fn ensure(
    ok: bool,
    field: &'static str,
    reason: impl FnOnce() -> String,
) -> Result<(), ValidationError> {
    if ok {
        Ok(())
    } else {
        Err(ValidationError {
            field,
            reason: reason(),
        })
    }
}

/// Rejects empty and whitespace-only text.
fn not_blank(field: &'static str, value: &str) -> Result<(), ValidationError> {
    ensure(!value.trim().is_empty(), field, || EMPTY.into())
}

fn not_empty_path(field: &'static str, path: &Path) -> Result<(), ValidationError> {
    ensure(!path.as_os_str().is_empty(), field, || EMPTY.into())
}

/// `None` disables the feature, `Some("")` is rejected as a likely mistake.
fn optional_text(field: &'static str, value: Option<&str>) -> Result<(), ValidationError> {
    value.map_or(Ok(()), |value| {
        ensure(!value.trim().is_empty(), field, || EMPTY_OPTIONAL.into())
    })
}

fn optional_path(field: &'static str, path: Option<&Path>) -> Result<(), ValidationError> {
    path.map_or(Ok(()), |path| {
        ensure(!path.as_os_str().is_empty(), field, || {
            EMPTY_OPTIONAL.into()
        })
    })
}
