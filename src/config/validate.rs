use std::{ops::RangeInclusive, path::Path};

use humantime::format_duration;

use crate::{
    audio::MAX_BUFFER,
    template::{Placeholder, Template},
};

use super::{
    AudioConfig, ChannelRef, Config, Destination, Event, EventVolume, EventsConfig, Sound,
    TeamspeakConfig, Volume,
};

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
    events(&config.events, config.audio.max_volume)?;

    not_empty_path("paths.state_dir", &config.paths.state_dir)?;
    not_empty_path("paths.secrets_file", &config.paths.secrets_file)?;
    not_empty_path("paths.ffmpeg_path", &config.paths.ffmpeg_path)?;
    not_empty_path(
        "sources.youtube.yt_dlp_path",
        &config.sources.youtube.yt_dlp_path,
    )?;
    // Joined into one `--extractor-args` value, so `;` or `=` could add other yt-dlp options.
    ensure(
        config.sources.youtube.player_clients.iter().all(|client| {
            !client.is_empty()
                && client
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        }),
        "sources.youtube.player_clients",
        || "names may only contain a-z, 0-9 and _".into(),
    )?;
    optional_path(
        "sources.local.music_dir",
        config.sources.local.music_dir.as_deref(),
    )?;
    optional_path(
        "sources.youtube.cookies_file",
        config.sources.youtube.cookies_file.as_deref(),
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
    nickname_len("teamspeak.nickname", &ts.nickname)?;

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

    ensure(audio.max_buffer <= MAX_BUFFER, "audio.max_buffer", || {
        format!("must not be greater than {}", format_duration(MAX_BUFFER))
    })?;
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

fn events(events: &EventsConfig, max_volume: Volume) -> Result<(), ValidationError> {
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

    event(&events.on_connect, fields!("on_connect"), max_volume, false)?;
    event(&events.on_idle, fields!("on_idle"), max_volume, false)?;
    event(&events.on_alone, fields!("on_alone"), max_volume, false)?;
    event(&events.on_party, fields!("on_party"), max_volume, false)?;
    event(
        &events.on_song_start,
        fields!("on_song_start"),
        max_volume,
        true,
    )?;
    optional_text(
        "events.on_disconnect.message",
        events.on_disconnect.message.as_deref(),
    )
}

/// The full names of the fields of one event, errors need them `'static`.
struct EventFields {
    set_volume: &'static str,
    message: &'static str,
    sound: &'static str,
    move_to: &'static str,
    description: &'static str,
    nickname: &'static str,
}

/// `EventFields` of `[events.<name>]`.
macro_rules! fields {
    ($name:literal) => {
        EventFields {
            set_volume: concat!("events.", $name, ".set_volume"),
            message: concat!("events.", $name, ".message"),
            sound: concat!("events.", $name, ".sound"),
            move_to: concat!("events.", $name, ".move_to"),
            description: concat!("events.", $name, ".description"),
            nickname: concat!("events.", $name, ".nickname"),
        }
    };
}
use fields;

/// `song_start` is `on_song_start`: the only event with a song for `{song}` and `{source}`,
/// and the only one without a sound, its song already plays.
/// An empty `description` is valid, it clears the description. `set_volume` has the limit
/// of `!bot volume`.
fn event(
    event: &Event,
    fields: EventFields,
    max_volume: Volume,
    song_start: bool,
) -> Result<(), ValidationError> {
    if let Some(EventVolume::Set(volume)) = event.set_volume {
        ensure(volume <= max_volume, fields.set_volume, || {
            format!(
                "must not be greater than audio.max_volume ({} > {})",
                volume.get(),
                max_volume.get()
            )
        })?;
    }
    optional_text(fields.message, event.message.as_ref().map(Template::as_str))?;
    placeholders(fields.message, event.message.as_ref(), song_start, false)?;
    match &event.sound {
        Some(_) if song_start => {
            return Err(ValidationError {
                field: fields.sound,
                reason: "on_song_start cannot play a sound, the song already plays".into(),
            });
        }
        Some(Sound::File(path)) => optional_path(fields.sound, Some(path))?,
        _ => {}
    }
    if let Some(Destination::Channel(ChannelRef::Name(name))) = &event.move_to {
        not_blank(fields.move_to, name)?;
    }
    placeholders(
        fields.description,
        event.description.as_ref(),
        song_start,
        true,
    )?;
    if let Some(name) = &event.nickname {
        optional_text(fields.nickname, Some(name.as_str()))?;
        placeholders(fields.nickname, Some(name), song_start, true)?;
        // With placeholders the length is known only when the event runs, the bot cuts it then.
        if name.placeholders().next().is_none() {
            nickname_len(fields.nickname, name.as_str())?;
        }
    }
    Ok(())
}

/// `{song}` and `{source}` need the song of `on_song_start`, `{default}` needs a field with
/// a value in `[teamspeak]`.
fn placeholders(
    field: &'static str,
    text: Option<&Template>,
    song: bool,
    default: bool,
) -> Result<(), ValidationError> {
    for placeholder in text.into_iter().flat_map(Template::placeholders) {
        let (allowed, only) = match placeholder {
            Placeholder::Song | Placeholder::Source => (song, "in on_song_start"),
            Placeholder::Default => (default, "in description and nickname"),
        };
        ensure(allowed, field, || {
            format!("{{{}}} works only {only}", placeholder.name())
        })?;
    }
    Ok(())
}

fn nickname_len(field: &'static str, nickname: &str) -> Result<(), ValidationError> {
    let len = nickname.chars().count();
    ensure(NICKNAME_LEN.contains(&len), field, || {
        format!(
            "must be {} to {} characters long (has {len})",
            NICKNAME_LEN.start(),
            NICKNAME_LEN.end()
        )
    })
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
