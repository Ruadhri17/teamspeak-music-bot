use std::{
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use tempfile::TempDir;
use url::Url;

use crate::{
    audio::AudioInput,
    config::YoutubeConfig,
    sources::{
        Location, QueryType, TrackRef,
        youtube::{
            InvalidVideoId, LinkError, VideoId, YoutubeError, YoutubeLink, open, parse_link,
            parse_output, resolve,
        },
    },
};

fn parse(text: &str) -> Result<VideoId, InvalidVideoId> {
    text.parse()
}

#[test]
fn dash_and_underscore_are_valid_anywhere() {
    assert!(parse("-abc_def-gh").is_ok());
    assert!(parse("___________").is_ok());
}

#[test]
fn wrong_length_is_invalid() {
    assert_eq!(parse(""), Err(InvalidVideoId));
    assert_eq!(parse("5NV6Rdv1a3"), Err(InvalidVideoId));
    assert_eq!(parse("5NV6Rdv1a3Ix"), Err(InvalidVideoId));
}

#[test]
fn characters_outside_the_alphabet_are_invalid() {
    for text in [
        "5NV6Rdv1a3 ",
        "5NV6Rdv1a3/",
        "5NV6Rdv1a3?",
        "5NV6Rdv1a3=",
        "5NV6Rdv1a3.",
    ] {
        assert_eq!(parse(text), Err(InvalidVideoId), "{text:?}");
    }
}

#[test]
fn non_ascii_with_eleven_bytes_is_invalid() {
    let text = "ą5NV6Rdv1a";
    assert_eq!(text.len(), 11);
    assert_eq!(parse(text), Err(InvalidVideoId));
}

#[test]
fn url_points_to_the_watch_page() {
    let id = parse("5NV6Rdv1a3I").unwrap();
    assert_eq!(id.url(), "https://www.youtube.com/watch?v=5NV6Rdv1a3I");
}

fn link(text: &str) -> Result<YoutubeLink, LinkError> {
    parse_link(&Url::parse(text).unwrap())
}

fn video(id: &str) -> Result<YoutubeLink, LinkError> {
    Ok(YoutubeLink::Video(parse(id).unwrap()))
}

#[test]
fn every_video_link_form_gives_the_id() {
    for text in [
        "https://www.youtube.com/watch?v=5NV6Rdv1a3I",
        "https://youtube.com/watch?v=5NV6Rdv1a3I",
        "https://m.youtube.com/watch?v=5NV6Rdv1a3I",
        "https://music.youtube.com/watch?v=5NV6Rdv1a3I",
        "http://www.youtube.com/watch?v=5NV6Rdv1a3I",
        "https://WWW.YouTube.com/watch?v=5NV6Rdv1a3I",
        "https://youtu.be/5NV6Rdv1a3I",
        "https://www.youtube.com/shorts/5NV6Rdv1a3I",
        "https://www.youtube.com/shorts/5NV6Rdv1a3I/",
        "https://www.youtube.com/embed/5NV6Rdv1a3I",
        "https://www.youtube.com/live/5NV6Rdv1a3I",
    ] {
        assert_eq!(link(text), video("5NV6Rdv1a3I"), "{text}");
    }
}

#[test]
fn extra_parameters_and_fragments_are_ignored() {
    for text in [
        "https://www.youtube.com/watch?list=PLxyz&v=5NV6Rdv1a3I&t=42s",
        "https://www.youtube.com/watch?v=5NV6Rdv1a3I&si=abc&index=3#t=42",
        "https://youtu.be/5NV6Rdv1a3I?si=abc&t=42",
    ] {
        assert_eq!(link(text), video("5NV6Rdv1a3I"), "{text}");
    }
}

#[test]
fn encoded_id_is_decoded() {
    assert_eq!(
        link("https://www.youtube.com/watch?v=%35NV6Rdv1a3I"),
        video("5NV6Rdv1a3I")
    );
}

#[test]
fn links_without_a_video_are_no_video() {
    for text in [
        "https://www.youtube.com/",
        "https://www.youtube.com/watch",
        "https://www.youtube.com/watch?list=PLxyz",
        "https://www.youtube.com/watchlater?v=5NV6Rdv1a3I",
        "https://www.youtube.com/@channel",
        "https://www.youtube.com/results?search_query=get+lucky",
        "https://www.youtube.com/shorts/",
        "https://youtu.be/",
    ] {
        assert_eq!(link(text), Err(LinkError::NoVideo), "{text}");
    }
}

#[test]
fn playlist_links_are_not_supported_yet() {
    for text in [
        "https://www.youtube.com/playlist?list=PLxyz",
        "https://music.youtube.com/playlist?list=OLAK5uy_abc",
    ] {
        assert_eq!(link(text), Err(LinkError::PlaylistNotSupported), "{text}");
    }
}

#[test]
fn invalid_id_in_a_link_is_rejected() {
    for text in [
        "https://www.youtube.com/watch?v=short",
        "https://www.youtube.com/watch?v=5NV6Rdv1a3I-extra",
        "https://youtu.be/watch",
        "https://www.youtube.com/shorts/bad%20id%20xx",
    ] {
        assert_eq!(
            link(text),
            Err(LinkError::InvalidId(InvalidVideoId)),
            "{text}"
        );
    }
}

#[test]
fn other_hosts_are_not_youtube() {
    for text in [
        "https://evil.com/watch?v=5NV6Rdv1a3I",
        "https://youtube.com.evil.net/watch?v=5NV6Rdv1a3I",
        "https://notyoutube.com/watch?v=5NV6Rdv1a3I",
        "https://youtu.be.evil.net/5NV6Rdv1a3I",
        "https://127.0.0.1/watch?v=5NV6Rdv1a3I",
    ] {
        assert_eq!(link(text), Err(LinkError::NotYoutube), "{text}");
    }
}

const VIDEO_JSON: &str = r#"{
    "_type": "video",
    "id": "5NV6Rdv1a3I",
    "title": "Daft Punk - Get Lucky",
    "duration": 249,
    "channel": "Daft Punk",
    "live_status": "not_live"
}"#;

fn track(id: &str, title: &str) -> TrackRef {
    TrackRef {
        location: Location::Youtube(id.parse().unwrap()),
        title: title.to_owned(),
    }
}

fn search_json(entries: &str) -> String {
    format!(
        r#"{{"_type": "playlist", "id": "daft punk", "title": "daft punk", "entries": [{entries}]}}"#
    )
}

fn entry(id: &str, title: &str) -> String {
    format!(
        r#"{{"_type": "url", "ie_key": "Youtube", "id": "{id}", "title": "{title}", "url": "https://www.youtube.com/watch?v={id}"}}"#
    )
}

#[test]
fn video_output_gives_its_track() {
    let track_ref = parse_output(VIDEO_JSON.as_bytes()).unwrap();
    assert_eq!(track_ref, track("5NV6Rdv1a3I", "Daft Punk - Get Lucky"));
}

#[test]
fn search_output_gives_the_first_entry() {
    let json = search_json(&format!(
        "{},{}",
        entry("5NV6Rdv1a3I", "first"),
        entry("-abc_def-gh", "second")
    ));
    assert_eq!(
        parse_output(json.as_bytes()).unwrap(),
        track("5NV6Rdv1a3I", "first")
    );
}

#[test]
fn empty_search_is_no_results() {
    let result = parse_output(search_json("").as_bytes());
    assert!(matches!(result, Err(YoutubeError::NoResults)), "{result:?}");
}

#[test]
fn title_keeps_unicode_and_escapes() {
    let json = search_json(&entry("5NV6Rdv1a3I", r#"Kult - Arahja \"live\" żółć 🎵"#));
    assert_eq!(
        parse_output(json.as_bytes()).unwrap().title,
        "Kult - Arahja \"live\" żółć 🎵"
    );
}

#[test]
fn invalid_id_in_output_is_rejected() {
    for id in [
        "short",
        "5NV6Rdv1a3I-too-long",
        "5NV6Rdv1a3/",
        "../../etc/x",
    ] {
        let json = search_json(&entry(id, "x"));
        let result = parse_output(json.as_bytes());
        assert!(
            matches!(result, Err(YoutubeError::InvalidId(_))),
            "{id}: {result:?}"
        );
    }
}

#[test]
fn unknown_shapes_are_json_errors() {
    for json in [
        r#"{"_type": "channel", "id": "x", "title": "x"}"#,
        r#"{"id": "5NV6Rdv1a3I", "title": "no type"}"#,
        r#"{"_type": "video", "id": "5NV6Rdv1a3I"}"#,
        r#"{"_type": "playlist", "id": "x"}"#,
        r#"{"_type": "video", "id": 123, "title": "x"}"#,
        r#"{"_type": "video", "id": "5NV6Rdv1a3I", "title""#,
        "",
        "not json",
    ] {
        let result = parse_output(json.as_bytes());
        assert!(
            matches!(result, Err(YoutubeError::Json(_))),
            "{json}: {result:?}"
        );
    }
}

#[test]
fn invalid_utf8_is_a_json_error() {
    let mut json = br#"{"_type": "video", "id": "5NV6Rdv1a3I", "title": ""#.to_vec();
    json.extend_from_slice(&[0xFF, 0xFE]);
    json.extend_from_slice(br#""}"#);
    let result = parse_output(&json);
    assert!(matches!(result, Err(YoutubeError::Json(_))), "{result:?}");
}

#[test]
fn error_messages_do_not_leak_details() {
    let err = parse_output(b"not json").unwrap_err();
    assert_eq!(err.to_string(), "YouTube lookup failed");
}

#[tokio::test]
#[ignore = "runs yt-dlp and needs the network"]
async fn real_search_finds_a_video() {
    let query = QueryType::Phrase("daft punk get lucky".to_owned());
    let track_ref = resolve(&config(Path::new("yt-dlp")), &query).await.unwrap();
    assert!(matches!(track_ref.location, Location::Youtube(_)));
    assert!(!track_ref.title.is_empty());
}

#[tokio::test]
#[ignore = "runs yt-dlp and needs the network"]
async fn real_link_gives_its_video() {
    let url = Url::parse("https://youtu.be/5NV6Rdv1a3I").unwrap();
    let track_ref = resolve(&config(Path::new("yt-dlp")), &QueryType::Link(url))
        .await
        .unwrap();
    assert_eq!(
        track_ref.location,
        Location::Youtube("5NV6Rdv1a3I".parse().unwrap())
    );
}

#[tokio::test]
async fn missing_yt_dlp_is_a_spawn_error() {
    let query = QueryType::Phrase("x".to_owned());
    let result = resolve(&config(Path::new("/nonexistent/yt-dlp")), &query).await;
    assert!(matches!(result, Err(YoutubeError::Spawn(_))), "{result:?}");
}

fn fake_yt_dlp(script: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("yt-dlp");
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    (dir, path)
}

fn config(yt_dlp: &Path) -> YoutubeConfig {
    YoutubeConfig {
        yt_dlp_path: yt_dlp.to_path_buf(),
        ..YoutubeConfig::default()
    }
}

fn open_stream(yt_dlp: &Path) -> Box<dyn Read + Send> {
    open_with(&config(yt_dlp))
}

fn open_with(config: &YoutubeConfig) -> Box<dyn Read + Send> {
    let id = "5NV6Rdv1a3I".parse().unwrap();
    match open(config, &id).unwrap() {
        AudioInput::Stream(reader) => reader,
        AudioInput::File(_) => panic!("expected a stream"),
    }
}

#[test]
fn open_streams_the_stdout_of_yt_dlp() {
    let (_dir, path) = fake_yt_dlp("printf audio-bytes");
    let mut data = Vec::new();
    open_stream(&path).read_to_end(&mut data).unwrap();
    assert_eq!(data, b"audio-bytes");
}

#[test]
fn open_passes_the_url_after_double_dash() {
    let (_dir, path) = fake_yt_dlp(r#"for a; do printf '%s\n' "$a"; done"#);
    let mut text = String::new();
    open_stream(&path).read_to_string(&mut text).unwrap();
    let args: Vec<&str> = text.lines().collect();
    assert_eq!(
        &args[args.len() - 2..],
        ["--", "https://www.youtube.com/watch?v=5NV6Rdv1a3I"]
    );
}

#[test]
fn failed_yt_dlp_is_a_read_error() {
    let (dir, path) = fake_yt_dlp(&logged(
        "printf partial; echo 'ERROR: video unavailable' >&2; exit 1",
    ));
    let mut data = Vec::new();
    let err = open_stream(&path).read_to_end(&mut data).unwrap_err();
    assert_eq!(data, b"partial");
    assert!(err.to_string().contains("yt-dlp exited"), "{err}");
    assert_eq!(runs(&dir).len(), 1);
}

#[test]
fn chatty_stderr_does_not_block_the_stream() {
    let (_dir, path) = fake_yt_dlp("head -c 1000000 /dev/zero >&2; printf done");
    let mut data = Vec::new();
    open_stream(&path).read_to_end(&mut data).unwrap();
    assert_eq!(data, b"done");
}

#[test]
fn dropping_the_stream_kills_yt_dlp() {
    let (dir, path) = fake_yt_dlp(r#"echo $$ > "$(dirname "$0")/pid"; printf x; exec sleep 30"#);
    let mut reader = open_stream(&path);
    let mut byte = [0u8; 1];
    reader.read_exact(&mut byte).unwrap();
    let pid = fs::read_to_string(dir.path().join("pid")).unwrap();

    let start = Instant::now();
    drop(reader);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(!Path::new(&format!("/proc/{}", pid.trim())).exists());
}

#[test]
fn missing_yt_dlp_cannot_be_opened() {
    let id = "5NV6Rdv1a3I".parse().unwrap();
    let result = open(&config(Path::new("/nonexistent/yt-dlp")), &id);
    assert!(matches!(result, Err(YoutubeError::Spawn(_))));
}

fn logged(body: &str) -> String {
    format!(r#"d="$(dirname "$0")"; echo "$*" >> "$d/runs"; run=$(wc -l < "$d/runs"); {body}"#)
}

fn runs(dir: &TempDir) -> Vec<String> {
    fs::read_to_string(dir.path().join("runs"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn failure_before_any_audio_is_retried_with_default_clients() {
    let (dir, path) = fake_yt_dlp(&logged(r#"[ "$run" -ge 2 ] || exit 1; printf audio"#));
    let mut data = Vec::new();
    open_stream(&path).read_to_end(&mut data).unwrap();
    assert_eq!(data, b"audio");

    let runs = runs(&dir);
    assert_eq!(runs.len(), 2);
    assert!(runs[0].contains("--extractor-args youtube:player_client=visionos"));
    assert!(!runs[1].contains("--extractor-args"));
}

#[test]
fn open_gives_up_after_three_attempts() {
    let (dir, path) = fake_yt_dlp(&logged("exit 1"));
    let mut data = Vec::new();
    let err = open_stream(&path).read_to_end(&mut data).unwrap_err();
    assert!(err.to_string().contains("yt-dlp exited"), "{err}");
    assert!(data.is_empty());
    assert_eq!(runs(&dir).len(), 3);
}

#[test]
fn several_player_clients_are_joined() {
    let (dir, path) = fake_yt_dlp(&logged("printf x"));
    let config = YoutubeConfig {
        player_clients: vec!["visionos".into(), "tv_samsung".into()],
        ..config(&path)
    };
    open_with(&config).read_to_end(&mut Vec::new()).unwrap();
    assert!(runs(&dir)[0].contains("youtube:player_client=visionos,tv_samsung"));
}

#[test]
fn empty_player_clients_leave_the_choice_to_yt_dlp() {
    let (dir, path) = fake_yt_dlp(&logged("exit 1"));
    let config = YoutubeConfig {
        player_clients: Vec::new(),
        ..config(&path)
    };
    assert!(open_with(&config).read_to_end(&mut Vec::new()).is_err());
    let runs = runs(&dir);
    assert_eq!(runs.len(), 3);
    assert!(runs.iter().all(|run| !run.contains("--extractor-args")));
}

#[tokio::test]
async fn failed_lookup_is_retried_with_default_clients() {
    let script = format!(
        r#"[ "$run" -ge 2 ] || exit 1; cat <<'JSON'
{VIDEO_JSON}
JSON"#
    );
    let (dir, path) = fake_yt_dlp(&logged(&script));
    let query = QueryType::Phrase("get lucky".to_owned());
    let track_ref = resolve(&config(&path), &query).await.unwrap();
    assert_eq!(track_ref, track("5NV6Rdv1a3I", "Daft Punk - Get Lucky"));

    let runs = runs(&dir);
    assert_eq!(runs.len(), 2);
    assert!(runs[0].contains("youtube:player_client=visionos"));
    assert!(!runs[1].contains("--extractor-args"));
    assert!(runs[1].ends_with("-- ytsearch1:get lucky"));
}

#[tokio::test]
async fn lookup_without_player_clients_is_not_retried() {
    let (dir, path) = fake_yt_dlp(&logged("exit 1"));
    let config = YoutubeConfig {
        player_clients: Vec::new(),
        ..config(&path)
    };
    let query = QueryType::Phrase("x".to_owned());
    let result = resolve(&config, &query).await;
    assert!(
        matches!(result, Err(YoutubeError::Failed { .. })),
        "{result:?}"
    );
    assert_eq!(runs(&dir).len(), 1);
}

#[tokio::test]
async fn bad_output_is_not_retried() {
    let (dir, path) = fake_yt_dlp(&logged("printf 'not json'"));
    let query = QueryType::Phrase("x".to_owned());
    let result = resolve(&config(&path), &query).await;
    assert!(matches!(result, Err(YoutubeError::Json(_))), "{result:?}");
    assert_eq!(runs(&dir).len(), 1);
}

#[test]
#[ignore = "runs yt-dlp and needs the network"]
fn real_open_streams_audio() {
    let mut reader = open_stream(Path::new("yt-dlp"));
    let mut data = vec![0u8; 100_000];
    reader.read_exact(&mut data).unwrap();
    assert_eq!(&data[..4], [0x1A, 0x45, 0xDF, 0xA3]);
}
