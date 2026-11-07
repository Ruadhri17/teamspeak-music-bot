use std::{num::NonZeroUsize, time::Duration};

use crate::config::{location::TEMPLATE, *};

fn parse(toml: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(toml)
}

#[test]
fn template_matches_default() {
    let parsed = parse(TEMPLATE).expect("template must parse");
    assert_eq!(parsed, Config::default());
}

#[test]
fn empty_file_gives_defaults() {
    assert_eq!(parse("").unwrap(), Config::default());
}

#[test]
fn missing_fields_keep_defaults() {
    let config = parse("[audio]\nbitrate = 128\n").unwrap();
    let mut expected = Config::default();
    expected.audio.bitrate = Bitrate::new(128).unwrap();
    assert_eq!(config, expected);
}

#[test]
fn default_survives_toml_round_trip() {
    let written = toml::to_string(&Config::default()).unwrap();
    assert_eq!(parse(&written).unwrap(), Config::default());
}

#[test]
fn unknown_keys_are_rejected_in_every_section() {
    let sections = [
        "teamspeak",
        "audio",
        "events",
        "events.on_connect",
        "events.on_disconnect",
        "events.on_idle",
        "events.on_alone",
        "events.on_party",
        "events.on_song_start",
        "queue",
        "permissions",
        "sources",
        "sources.tidal",
        "sources.youtube",
        "sources.local",
        "paths",
        "logging",
    ];
    for section in sections {
        let toml = format!("[{section}]\nunknown_key = 1\n");
        assert!(parse(&toml).is_err(), "[{section}] accepted an unknown key");
    }
    assert!(
        parse("unknown_key = 1\n").is_err(),
        "top level accepted an unknown key"
    );
}

#[test]
fn channel_ref_accepts_id_and_name() {
    let config = parse("[teamspeak]\nhome_channel = 22\n").unwrap();
    assert_eq!(config.teamspeak.home_channel, Some(ChannelRef::Id(22)));

    let config = parse("[teamspeak]\nhome_channel = \"Music/Rock\"\n").unwrap();
    assert_eq!(
        config.teamspeak.home_channel,
        Some(ChannelRef::Name("Music/Rock".into()))
    );
}

#[test]
fn channel_ref_rejects_other_types() {
    assert!(parse("[teamspeak]\nhome_channel = true\n").is_err());
    assert!(parse("[teamspeak]\nhome_channel = -1\n").is_err());
}

#[test]
fn group_ref_accepts_ids_and_names_in_one_list() {
    let config = parse("[permissions]\nadmin_groups = [6, \"Admin\"]\n").unwrap();
    assert_eq!(
        config.permissions.admin_groups,
        [GroupRef::Id(6), GroupRef::Name("Admin".into())]
    );
}

#[test]
fn empty_list_replaces_default_groups() {
    let config = parse("[permissions]\nadmin_groups = []\n").unwrap();
    assert!(config.permissions.admin_groups.is_empty());
}

#[test]
fn enums_use_lowercase_names() {
    let config = parse(
        "[sources]\ndefault = \"youtube\"\n\n[sources.tidal]\nquality = \"lossless\"\n\n[logging]\nlevel = \"trace\"\n",
    )
    .unwrap();
    assert_eq!(config.sources.default, SourceKind::Youtube);
    assert_eq!(config.sources.tidal.quality, TidalQuality::Lossless);
    assert_eq!(config.logging.level, LogLevel::Trace);
}

#[test]
fn enums_reject_unknown_names() {
    assert!(
        parse("[sources.tidal]\nquality = \"Lossless\"\n").is_err(),
        "case matters"
    );
    assert!(parse("[sources]\ndefault = \"deezer\"\n").is_err());
    assert!(parse("[logging]\nlevel = \"verbose\"\n").is_err());
}

#[test]
fn durations_use_human_format() {
    let config =
        parse("[audio]\nstart_buffer = \"1s 500ms\"\npause_release_after = \"1h\"\n").unwrap();
    assert_eq!(config.audio.start_buffer, Duration::from_millis(1500));
    assert_eq!(config.audio.pause_release_after, Duration::from_secs(3600));
}

#[test]
fn durations_reject_plain_numbers_and_bad_units() {
    assert!(
        parse("[audio]\nstart_buffer = 3\n").is_err(),
        "unit is required"
    );
    assert!(parse("[audio]\nstart_buffer = \"3 parsecs\"\n").is_err());
}

#[test]
fn queue_max_len_is_optional() {
    assert_eq!(parse("").unwrap().queue.max_len, None);
    let config = parse("[queue]\nmax_len = 100\n").unwrap();
    assert_eq!(config.queue.max_len, NonZeroUsize::new(100));
}

#[test]
fn queue_sizes_reject_zero() {
    assert!(parse("[queue]\nmax_len = 0\n").is_err());
    assert!(parse("[queue]\nshow_page_size = 0\n").is_err());
}
