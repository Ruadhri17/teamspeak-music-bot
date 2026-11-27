use std::{path::PathBuf, time::Duration};

use crate::{
    config::{ChannelRef, Config, Destination, EventVolume, Sound, Volume, validate::validate},
    template::Template,
};

fn text(text: &str) -> Option<Template> {
    Some(text.parse().unwrap())
}

fn with_volumes(default: u8, max: u8) -> Config {
    let mut config = Config::default();
    config.audio.default_volume = Volume::new(default).unwrap();
    config.audio.max_volume = Volume::new(max).unwrap();
    config
}

fn with_buffers(start: u64, max: u64) -> Config {
    let mut config = Config::default();
    config.audio.start_buffer = Duration::from_secs(start);
    config.audio.max_buffer = Duration::from_secs(max);
    config
}

fn with_nickname(nickname: &str) -> Config {
    let mut config = Config::default();
    config.teamspeak.nickname = nickname.into();
    config
}

fn broken_field(config: &Config) -> &'static str {
    validate(config)
        .expect_err("config should be invalid")
        .field
}

type Case = (&'static str, fn(&mut Config));

fn assert_each_rejected(cases: &[Case]) {
    for (field, break_config) in cases {
        let mut config = Config::default();
        break_config(&mut config);
        assert_eq!(broken_field(&config), *field);
    }
}

#[test]
fn default_config_is_valid() {
    assert!(validate(&Config::default()).is_ok());
}

#[test]
fn default_volume_may_equal_max_volume() {
    assert!(validate(&with_volumes(50, 50)).is_ok());
    assert!(validate(&with_volumes(0, 0)).is_ok());
}

#[test]
fn default_volume_above_max_volume_is_rejected() {
    let err = validate(&with_volumes(80, 50)).unwrap_err();
    assert_eq!(err.field, "audio.default_volume");
    assert_eq!(
        err.to_string(),
        "audio.default_volume: must not be greater than audio.max_volume (80 > 50)"
    );
}

#[test]
fn start_buffer_may_equal_max_buffer_or_be_zero() {
    assert!(validate(&with_buffers(5, 5)).is_ok());
    assert!(validate(&with_buffers(0, 5)).is_ok());
}

#[test]
fn start_buffer_above_max_buffer_is_rejected() {
    let err = validate(&with_buffers(10, 5)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "audio.start_buffer: must not be greater than audio.max_buffer (10s > 5s)"
    );
}

#[test]
fn zero_max_buffer_is_rejected() {
    assert_eq!(broken_field(&with_buffers(0, 0)), "audio.max_buffer");
}

#[test]
fn nickname_length_bounds() {
    assert!(validate(&with_nickname("abc")).is_ok());
    assert!(validate(&with_nickname(&"a".repeat(30))).is_ok());
    assert!(
        validate(&with_nickname(&"ą".repeat(30))).is_ok(),
        "counts characters, not bytes"
    );

    for nickname in ["", "   ", "ab", &"a".repeat(31)] {
        assert_eq!(
            broken_field(&with_nickname(nickname)),
            "teamspeak.nickname",
            "{nickname:?}"
        );
    }
}

#[test]
fn nickname_length_error_message() {
    let err = validate(&with_nickname("ab")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "teamspeak.nickname: must be 3 to 30 characters long (has 2)"
    );
}

#[test]
fn home_channel_id_and_name_are_valid() {
    let mut config = Config::default();
    config.teamspeak.home_channel = Some(ChannelRef::Id(22));
    assert!(validate(&config).is_ok());
    config.teamspeak.home_channel = Some(ChannelRef::Name("Music/Rock".into()));
    assert!(validate(&config).is_ok());
}

#[test]
fn empty_required_values_are_rejected() {
    assert_each_rejected(&[
        ("teamspeak.address", |c| c.teamspeak.address = String::new()),
        ("teamspeak.address", |c| c.teamspeak.address = "  ".into()),
        ("teamspeak.identity_file", |c| {
            c.teamspeak.identity_file = PathBuf::new()
        }),
        ("teamspeak.home_channel", |c| {
            c.teamspeak.home_channel = Some(ChannelRef::Name(" ".into()))
        }),
        ("paths.state_dir", |c| c.paths.state_dir = PathBuf::new()),
        ("paths.secrets_file", |c| {
            c.paths.secrets_file = PathBuf::new()
        }),
        ("paths.ffmpeg_path", |c| {
            c.paths.ffmpeg_path = PathBuf::new()
        }),
        ("sources.youtube.yt_dlp_path", |c| {
            c.sources.youtube.yt_dlp_path = PathBuf::new()
        }),
    ]);
}

#[test]
fn empty_optional_values_are_rejected() {
    assert_each_rejected(&[
        ("sources.local.music_dir", |c| {
            c.sources.local.music_dir = Some(PathBuf::new())
        }),
        ("sources.youtube.cookies_file", |c| {
            c.sources.youtube.cookies_file = Some(PathBuf::new())
        }),
        ("events.on_idle.sound", |c| {
            c.events.on_idle.sound = Some(Sound::File(PathBuf::new()))
        }),
        ("events.on_alone.sound", |c| {
            c.events.on_alone.sound = Some(Sound::File(PathBuf::new()))
        }),
        ("events.on_party.sound", |c| {
            c.events.on_party.sound = Some(Sound::File(PathBuf::new()))
        }),
        ("events.on_connect.message", |c| {
            c.events.on_connect.message = text("")
        }),
        ("events.on_disconnect.message", |c| {
            c.events.on_disconnect.message = Some(" ".into())
        }),
        ("events.on_idle.message", |c| {
            c.events.on_idle.message = text("")
        }),
        ("events.on_alone.message", |c| {
            c.events.on_alone.message = text("")
        }),
        ("events.on_party.message", |c| {
            c.events.on_party.message = text("")
        }),
        ("events.on_song_start.message", |c| {
            c.events.on_song_start.message = text("")
        }),
    ]);
}

#[test]
fn empty_optional_value_error_message_has_hint() {
    let mut config = Config::default();
    config.events.on_idle.message = text("");
    assert_eq!(
        validate(&config).unwrap_err().to_string(),
        "events.on_idle.message: must not be empty, remove the line to turn it off"
    );
}

#[test]
fn filled_optional_values_are_valid() {
    let mut config = Config::default();
    config.sources.local.music_dir = Some("music".into());
    config.events.on_idle.sound = Some(Sound::File("bye.wav".into()));
    config.events.on_idle.message = text("See you later");
    config.events.on_song_start.message = text("Now playing: {song} from {source}");
    assert!(validate(&config).is_ok());
}

#[test]
fn zero_durations_are_rejected_where_they_break_the_bot() {
    assert_each_rejected(&[
        ("events.on_idle.delay", |c| {
            c.events.on_idle.delay = Duration::ZERO
        }),
        ("events.on_alone.delay", |c| {
            c.events.on_alone.delay = Duration::ZERO
        }),
        ("logging.metrics_interval", |c| {
            c.logging.metrics_interval = Duration::ZERO
        }),
    ]);
}

#[test]
fn zero_party_delay_and_pause_release_are_valid() {
    let mut config = Config::default();
    config.events.on_party.delay = Duration::ZERO;
    config.audio.pause_release_after = Duration::ZERO;
    assert!(validate(&config).is_ok());
}

#[test]
fn player_clients_with_unsafe_names_are_rejected() {
    assert_each_rejected(&[
        ("sources.youtube.player_clients", |c| {
            c.sources.youtube.player_clients = vec![String::new()]
        }),
        ("sources.youtube.player_clients", |c| {
            c.sources.youtube.player_clients = vec!["visionos;po_token=x".into()]
        }),
        ("sources.youtube.player_clients", |c| {
            c.sources.youtube.player_clients = vec!["tv,web".into()]
        }),
        ("sources.youtube.player_clients", |c| {
            c.sources.youtube.player_clients = vec!["TV".into()]
        }),
    ]);
}

#[test]
fn player_clients_may_be_empty_or_several() {
    let mut config = Config::default();
    config.sources.youtube.player_clients = Vec::new();
    assert!(validate(&config).is_ok());
    config.sources.youtube.player_clients = vec!["visionos".into(), "tv_samsung".into()];
    assert!(validate(&config).is_ok());
}

#[test]
fn event_actions_are_checked() {
    assert_each_rejected(&[
        ("events.on_party.nickname", |c| {
            c.events.on_party.nickname = text("ab")
        }),
        ("events.on_idle.nickname", |c| {
            c.events.on_idle.nickname = text(" ")
        }),
        ("events.on_alone.move_to", |c| {
            c.events.on_alone.move_to = Some(Destination::Channel(ChannelRef::Name(" ".into())))
        }),
    ]);
}

#[test]
fn written_leaving_event_needs_its_delay() {
    let mut config = Config::default();
    config.events.on_alone = Default::default();
    assert_eq!(
        validate(&config).unwrap_err().to_string(),
        "events.on_alone.delay: must be greater than 0"
    );
}

#[test]
fn empty_event_description_clears_it() {
    let mut config = Config::default();
    config.events.on_party.description = text("");
    config.events.on_party.nickname = text("Music Bot");
    assert!(validate(&config).is_ok());
}

#[test]
fn max_buffer_has_an_upper_limit() {
    assert!(validate(&with_buffers(3, 60)).is_ok());
    let err = validate(&with_buffers(3, 61)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "audio.max_buffer: must not be greater than 1m"
    );
}

#[test]
fn placeholders_are_checked_against_their_event_and_field() {
    assert_each_rejected(&[
        ("events.on_party.message", |c| {
            c.events.on_party.message = text("Now playing: {song}")
        }),
        ("events.on_idle.description", |c| {
            c.events.on_idle.description = text("from {source}")
        }),
        ("events.on_song_start.message", |c| {
            c.events.on_song_start.message = text("{default}")
        }),
    ]);
}

#[test]
fn placeholder_errors_say_where_they_work() {
    let mut config = Config::default();
    config.events.on_connect.message = text("{default} is here");
    assert_eq!(
        validate(&config).unwrap_err().to_string(),
        "events.on_connect.message: {default} works only in description and nickname"
    );
    config.events.on_connect.message = text("Playing {song}");
    assert_eq!(
        validate(&config).unwrap_err().to_string(),
        "events.on_connect.message: {song} works only in on_song_start"
    );
}

#[test]
fn placeholders_where_they_fit_are_valid() {
    let mut config = Config::default();
    config.events.on_song_start.message = text("Now playing: {song} from {source}");
    config.events.on_song_start.description = text("{default} | {song}");
    config.events.on_song_start.nickname = text("{song}");
    config.events.on_idle.nickname = text("{default}");
    config.events.on_party.description = text("{default} (busy)");
    assert!(validate(&config).is_ok());
}

#[test]
fn song_start_cannot_play_a_sound() {
    let mut config = Config::default();
    config.events.on_song_start.sound = Some(Sound::File("jingle.wav".into()));
    assert_eq!(
        validate(&config).unwrap_err().to_string(),
        "events.on_song_start.sound: on_song_start cannot play a sound, the song already plays"
    );
}

#[test]
fn set_volume_may_reach_max_volume() {
    let mut config = with_volumes(50, 80);
    config.events.on_party.set_volume = Some(EventVolume::Set(Volume::new(80).unwrap()));
    config.events.on_idle.set_volume = Some(EventVolume::Default);
    assert!(validate(&config).is_ok());
}

#[test]
fn set_volume_above_max_volume_is_rejected() {
    let mut config = with_volumes(50, 80);
    config.events.on_song_start.set_volume = Some(EventVolume::Set(Volume::new(90).unwrap()));
    let err = validate(&config).unwrap_err();
    assert_eq!(err.field, "events.on_song_start.set_volume");
    assert_eq!(
        err.to_string(),
        "events.on_song_start.set_volume: must not be greater than audio.max_volume (90 > 80)"
    );
}

#[test]
fn show_page_size_is_at_most_thirty() {
    let mut config = Config::default();
    config.queue.show_page_size = std::num::NonZeroUsize::new(30).unwrap();
    assert!(validate(&config).is_ok());
    config.queue.show_page_size = std::num::NonZeroUsize::new(31).unwrap();
    assert_eq!(broken_field(&config), "queue.show_page_size");
}
