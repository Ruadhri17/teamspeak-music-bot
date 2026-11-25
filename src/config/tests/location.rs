use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::config::{
    Config, ConfigError,
    location::{ConfigLocation, TEMPLATE},
};

fn setup() -> (TempDir, ConfigLocation) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let location = ConfigLocation::new(dir.path().join("config.toml")).expect("valid path");
    (dir, location)
}

fn setup_with(content: &str) -> (TempDir, ConfigLocation) {
    let (dir, location) = setup();
    fs::write(location.file_path(), content).expect("write config");
    (dir, location)
}

fn read(location: &ConfigLocation) -> String {
    fs::read_to_string(location.file_path()).expect("read config")
}

fn cause(err: &ConfigError) -> String {
    std::error::Error::source(err)
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn resolved_default(dir: &Path) -> Config {
    let mut config = Config::default();
    config.paths.state_dir = dir.join("state");
    config.paths.secrets_file = dir.join("secrets.toml");
    config
}

#[test]
fn new_makes_relative_path_absolute() {
    let location = ConfigLocation::new("config.toml".into()).unwrap();
    let expected = std::env::current_dir().unwrap().join("config.toml");
    assert_eq!(location.file_path(), expected);
}

#[test]
fn new_keeps_absolute_path() {
    let location = ConfigLocation::new("/etc/tsmusic/config.toml".into()).unwrap();
    assert_eq!(location.file_path(), Path::new("/etc/tsmusic/config.toml"));
}

#[test]
fn new_rejects_root() {
    let err = ConfigLocation::new("/".into()).unwrap_err();
    assert!(
        matches!(&err, ConfigError::InvalidPath { path, .. } if path == Path::new("/")),
        "{err:?}"
    );
}

#[test]
fn new_rejects_empty_path() {
    let err = ConfigLocation::new(PathBuf::new()).unwrap_err();
    assert!(matches!(err, ConfigError::InvalidPath { .. }), "{err:?}");
}

#[test]
fn init_writes_template() {
    let (_dir, location) = setup();
    location.init(false).unwrap();
    assert_eq!(read(&location), TEMPLATE);
}

#[test]
fn init_refuses_to_overwrite_existing_file() {
    let (_dir, location) = setup_with("# my config");
    let err = location.init(false).unwrap_err();
    assert!(matches!(err, ConfigError::AlreadyExists { .. }), "{err:?}");
    assert_eq!(
        read(&location),
        "# my config",
        "existing file must not change"
    );
}

#[test]
fn init_force_overwrites_existing_file() {
    let (_dir, location) = setup_with("# my config");
    location.init(true).unwrap();
    assert_eq!(read(&location), TEMPLATE);
}

#[test]
fn init_force_creates_missing_file() {
    let (_dir, location) = setup();
    location.init(true).unwrap();
    assert_eq!(read(&location), TEMPLATE);
}

#[test]
fn init_fails_when_directory_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let location = ConfigLocation::new(dir.path().join("missing/config.toml")).unwrap();
    for force in [false, true] {
        let err = location.init(force).unwrap_err();
        assert!(
            matches!(err, ConfigError::Write { .. }),
            "force = {force}: {err:?}"
        );
    }
}

#[test]
fn load_after_init_gives_defaults() {
    let (dir, location) = setup();
    location.init(false).unwrap();
    assert_eq!(location.load().unwrap(), resolved_default(dir.path()));
}

#[test]
fn load_empty_file_gives_defaults() {
    let (dir, location) = setup_with("");
    assert_eq!(location.load().unwrap(), resolved_default(dir.path()));
}

#[test]
fn load_reads_values_and_keeps_other_defaults() {
    let (_dir, location) =
        setup_with("[teamspeak]\nnickname = \"DJ Bot\"\n\n[audio]\nbitrate = 128\n");
    let config = location.load().unwrap();
    assert_eq!(config.teamspeak.nickname, "DJ Bot");
    assert_eq!(config.audio.bitrate.get(), 128);
    assert_eq!(
        config.teamspeak.address,
        Config::default().teamspeak.address
    );
    assert_eq!(config.audio.complexity, Config::default().audio.complexity);
}

#[test]
fn load_missing_file() {
    let (_dir, location) = setup();
    let err = location.load().unwrap_err();
    assert!(
        matches!(&err, ConfigError::NotFound { path } if path == location.file_path()),
        "{err:?}"
    );
    assert!(
        err.to_string().contains("tsmusic init"),
        "hint missing: {err}"
    );
}

#[test]
fn load_unreadable_file() {
    let (_dir, location) = setup();
    fs::create_dir(location.file_path()).unwrap();
    let err = location.load().unwrap_err();
    assert!(matches!(err, ConfigError::Read { .. }), "{err:?}");
}

#[test]
fn load_invalid_toml() {
    let (_dir, location) = setup_with("[teamspeak\n");
    let err = location.load().unwrap_err();
    let ConfigError::Parse { path, .. } = &err else {
        panic!("expected Parse, got {err:?}");
    };
    assert_eq!(path, location.file_path());
}

#[test]
fn load_rejects_unknown_field() {
    let (_dir, location) = setup_with("[teamspeak]\nnick = \"DJ\"\n");
    let err = location.load().unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "{err:?}");
    assert!(
        cause(&err).contains("unknown field `nick`"),
        "{}",
        cause(&err)
    );
}

#[test]
fn load_rejects_unknown_section() {
    let (_dir, location) = setup_with("[teamspek]\naddress = \"localhost\"\n");
    let err = location.load().unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "{err:?}");
}

#[test]
fn load_rejects_wrong_type() {
    let (_dir, location) = setup_with("[audio]\nbitrate = \"fast\"\n");
    let err = location.load().unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "{err:?}");
}

#[test]
fn load_reports_out_of_range_value_with_line() {
    let (_dir, location) = setup_with("[audio]\nbitrate = 500\n");
    let err = location.load().unwrap_err();
    let cause = cause(&err);
    assert!(cause.contains("line 2"), "{cause}");
    assert!(
        cause.contains("bitrate must be between 32 and 192 kb/s"),
        "{cause}"
    );
}

#[test]
fn load_resolves_relative_paths_against_config_dir() {
    let (dir, location) = setup_with(
        r#"
[paths]
state_dir = "data/state"
secrets_file = "private/secrets.toml"

[events.on_idle]
sound = "sounds/bye.wav"

[events.on_alone]
sound = "alone.wav"

[events.on_party]
sound = "sounds/hi.wav"

[sources.local]
music_dir = "music"
"#,
    );
    let config = location.load().unwrap();
    let base = dir.path();
    assert_eq!(config.paths.state_dir, base.join("data/state"));
    assert_eq!(config.paths.secrets_file, base.join("private/secrets.toml"));
    assert_eq!(
        config.events.on_idle.sound,
        Some(base.join("sounds/bye.wav"))
    );
    assert_eq!(config.events.on_alone.sound, Some(base.join("alone.wav")));
    assert_eq!(
        config.events.on_party.sound,
        Some(base.join("sounds/hi.wav"))
    );
    assert_eq!(config.sources.local.music_dir, Some(base.join("music")));
}

#[test]
fn load_keeps_absolute_paths() {
    let (_dir, location) = setup_with(
        r#"
[paths]
state_dir = "/var/lib/tsmusic"

[events.on_idle]
sound = "/usr/share/tsmusic/bye.wav"

[sources.local]
music_dir = "/srv/music"
"#,
    );
    let config = location.load().unwrap();
    assert_eq!(config.paths.state_dir, Path::new("/var/lib/tsmusic"));
    assert_eq!(
        config.events.on_idle.sound.as_deref(),
        Some(Path::new("/usr/share/tsmusic/bye.wav"))
    );
    assert_eq!(
        config.sources.local.music_dir.as_deref(),
        Some(Path::new("/srv/music"))
    );
}

#[test]
fn load_keeps_missing_optional_paths_empty() {
    let (_dir, location) = setup_with("");
    let config = location.load().unwrap();
    assert_eq!(config.events.on_idle.sound, None);
    assert_eq!(config.events.on_alone.sound, None);
    assert_eq!(config.events.on_party.sound, None);
    assert_eq!(config.sources.local.music_dir, None);
}

#[test]
fn load_keeps_bare_program_names() {
    let (_dir, location) = setup_with(
        "[paths]\nffmpeg_path = \"ffmpeg\"\n\n[sources.youtube]\nyt_dlp_path = \"yt-dlp\"\n",
    );
    let config = location.load().unwrap();
    assert_eq!(config.paths.ffmpeg_path, Path::new("ffmpeg"));
    assert_eq!(config.sources.youtube.yt_dlp_path, Path::new("yt-dlp"));
}

#[test]
fn load_resolves_program_paths_with_directory() {
    let (dir, location) = setup_with(
        "[paths]\nffmpeg_path = \"./bin/ffmpeg\"\n\n[sources.youtube]\nyt_dlp_path = \"/usr/local/bin/yt-dlp\"\n",
    );
    let config = location.load().unwrap();
    assert_eq!(config.paths.ffmpeg_path, dir.path().join("./bin/ffmpeg"));
    assert_eq!(
        config.sources.youtube.yt_dlp_path,
        Path::new("/usr/local/bin/yt-dlp")
    );
}

#[test]
fn load_rejects_config_breaking_validation_rule() {
    let (_dir, location) = setup_with("[audio]\ndefault_volume = 80\nmax_volume = 50\n");
    let err = location.load().unwrap_err();
    assert!(
        matches!(&err, ConfigError::Invalid { path, .. } if path == location.file_path()),
        "{err:?}"
    );
    assert!(
        cause(&err).contains("audio.default_volume"),
        "{}",
        cause(&err)
    );
}
