use std::{
    fs,
    io::Read,
    num::NonZeroUsize,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use tempfile::TempDir;
use url::Url;

use crate::{
    audio::AudioInput,
    config::YoutubeConfig,
    process::Stopper,
    sources::{
        Location, Playlist, QueryType, TrackRef,
        youtube::{
            Clients, InvalidId, LinkError, PlaylistId, VideoId, YoutubeError, YoutubeLink, open,
            open_with, parse_link, parse_output, resolve, resolve_with,
        },
    },
};

const LIMIT: NonZeroUsize = NonZeroUsize::new(100).unwrap();

fn parse(text: &str) -> Result<VideoId, InvalidId> {
    text.parse()
}

#[test]
fn dash_and_underscore_are_valid_anywhere() {
    assert!(parse("-abc_def-gh").is_ok());
    assert!(parse("___________").is_ok());
}

#[test]
fn wrong_length_is_invalid() {
    assert_eq!(parse(""), Err(InvalidId));
    assert_eq!(parse("5NV6Rdv1a3"), Err(InvalidId));
    assert_eq!(parse("5NV6Rdv1a3Ix"), Err(InvalidId));
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
        assert_eq!(parse(text), Err(InvalidId), "{text:?}");
    }
}

#[test]
fn non_ascii_with_eleven_bytes_is_invalid() {
    let text = "ą5NV6Rdv1a";
    assert_eq!(text.len(), 11);
    assert_eq!(parse(text), Err(InvalidId));
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

fn playlist(id: &str) -> Result<YoutubeLink, LinkError> {
    Ok(YoutubeLink::Playlist(id.parse().unwrap()))
}

#[test]
fn playlist_links_give_the_playlist() {
    for (text, id) in [
        (
            "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
            "PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
        ),
        (
            "https://youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
            "PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
        ),
        (
            "https://m.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
            "PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
        ),
        (
            "https://music.youtube.com/playlist?list=OLAK5uy_nBqgVkx4qQ8n0aO3abcdefghijklmnopq",
            "OLAK5uy_nBqgVkx4qQ8n0aO3abcdefghijklmnopq",
        ),
        (
            "https://www.youtube.com/playlist?si=abc&list=UUuAXFkgsw1L7xaCfnd5JJOw#top",
            "UUuAXFkgsw1L7xaCfnd5JJOw",
        ),
    ] {
        assert_eq!(link(text), playlist(id), "{text}");
    }
}

#[test]
fn video_in_a_playlist_gives_only_the_video() {
    for text in [
        "https://www.youtube.com/watch?v=5NV6Rdv1a3I&list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI&index=5",
        "https://youtu.be/5NV6Rdv1a3I?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI",
        "https://music.youtube.com/watch?v=5NV6Rdv1a3I&list=OLAK5uy_nBqgVkx4qQ8n0aO3abcdefghijklmnopq",
        "https://www.youtube.com/watch?v=5NV6Rdv1a3I&list=RD5NV6Rdv1a3I",
    ] {
        assert_eq!(link(text), video("5NV6Rdv1a3I"), "{text}");
    }
}

#[test]
fn mixes_are_not_supported() {
    for text in [
        "https://www.youtube.com/playlist?list=RD5NV6Rdv1a3I",
        "https://music.youtube.com/playlist?list=RDAMVM5NV6Rdv1a3I",
    ] {
        assert_eq!(link(text), Err(LinkError::MixNotSupported), "{text}");
    }
}

#[test]
fn playlist_page_without_a_list_is_no_playlist() {
    for text in [
        "https://www.youtube.com/playlist",
        "https://www.youtube.com/playlist?v=5NV6Rdv1a3I",
    ] {
        assert_eq!(link(text), Err(LinkError::NoPlaylist), "{text}");
    }
}

#[test]
fn invalid_playlist_id_is_rejected() {
    for text in [
        "https://www.youtube.com/playlist?list=",
        "https://www.youtube.com/playlist?list=PL%20xyz",
        "https://www.youtube.com/playlist?list=PL.xyz",
        "https://www.youtube.com/playlist?list=PL%2Fxyz",
    ] {
        assert_eq!(link(text), Err(LinkError::InvalidId(InvalidId)), "{text}");
    }
}

#[test]
fn playlist_url_points_to_the_playlist_page() {
    let id: PlaylistId = "PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI".parse().unwrap();
    assert_eq!(
        id.url(),
        "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI"
    );
}

#[test]
fn playlist_id_length_is_limited() {
    assert_eq!("".parse::<PlaylistId>(), Err(InvalidId));
    assert_eq!("P".parse::<PlaylistId>(), Err(InvalidId));
    assert!("PL".parse::<PlaylistId>().is_ok());
    assert!("P".repeat(64).parse::<PlaylistId>().is_ok());
    assert_eq!("P".repeat(65).parse::<PlaylistId>(), Err(InvalidId));
}

#[test]
fn invalid_id_in_a_link_is_rejected() {
    for text in [
        "https://www.youtube.com/watch?v=short",
        "https://www.youtube.com/watch?v=5NV6Rdv1a3I-extra",
        "https://youtu.be/watch",
        "https://www.youtube.com/shorts/bad%20id%20xx",
    ] {
        assert_eq!(link(text), Err(LinkError::InvalidId(InvalidId)), "{text}");
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
    let found = parse_output(VIDEO_JSON.as_bytes()).unwrap();
    assert_eq!(
        found.tracks,
        [track("5NV6Rdv1a3I", "Daft Punk - Get Lucky")]
    );
    assert_eq!(found.playlist, None);
}

#[test]
fn playlist_output_gives_its_title_and_size() {
    let json = r#"{"_type": "playlist", "id": "PLx", "title": "Popular Music Videos", "playlist_count": 182, "entries": [{"_type": "url", "id": "fOT0BUpITw8", "title": "BELLAKEO"}]}"#;
    assert_eq!(
        parse_output(json.as_bytes()).unwrap().playlist,
        Some(Playlist {
            title: "Popular Music Videos".to_owned(),
            total: Some(182),
        })
    );
}

#[test]
fn playlist_output_without_a_size_or_title_is_accepted() {
    let json = format!(
        r#"{{"_type": "playlist", "entries": [{}]}}"#,
        entry("fOT0BUpITw8", "x")
    );
    assert_eq!(
        parse_output(json.as_bytes()).unwrap().playlist,
        Some(Playlist {
            title: String::new(),
            total: None,
        })
    );
}

#[test]
fn playlist_output_gives_every_entry_in_order() {
    let json = search_json(&format!(
        "{},{},{}",
        entry("5NV6Rdv1a3I", "first"),
        entry("-abc_def-gh", "second"),
        entry("fOT0BUpITw8", "third")
    ));
    assert_eq!(
        parse_output(json.as_bytes()).unwrap().tracks,
        [
            track("5NV6Rdv1a3I", "first"),
            track("-abc_def-gh", "second"),
            track("fOT0BUpITw8", "third"),
        ]
    );
}

#[test]
fn invalid_id_anywhere_in_a_playlist_rejects_it() {
    let json = search_json(&format!(
        "{},{}",
        entry("5NV6Rdv1a3I", "good"),
        entry("../../etc/x", "bad")
    ));
    let result = parse_output(json.as_bytes());
    assert!(
        matches!(result, Err(YoutubeError::InvalidId(_))),
        "{result:?}"
    );
}

#[test]
fn real_playlist_shape_is_understood() {
    let json = r#"{"_type": "playlist", "id": "PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI", "title": "Popular Music Videos", "playlist_count": 182, "availability": "public", "entries": [{"_type": "url", "ie_key": "Youtube", "id": "fOT0BUpITw8", "title": "BELLAKEO", "url": "https://www.youtube.com/watch?v=fOT0BUpITw8", "duration": 235, "live_status": null, "availability": null}]}"#;
    assert_eq!(
        parse_output(json.as_bytes()).unwrap().tracks,
        [track("fOT0BUpITw8", "BELLAKEO")]
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
        parse_output(json.as_bytes()).unwrap().tracks[0].title,
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
    let tracks = resolve(&config(Path::new("yt-dlp")), &query, LIMIT)
        .await
        .unwrap()
        .tracks;
    assert_eq!(tracks.len(), 1);
    assert!(matches!(tracks[0].location, Location::Youtube(_)));
    assert!(!tracks[0].title.is_empty());
}

#[tokio::test]
#[ignore = "runs yt-dlp and needs the network"]
async fn real_link_gives_its_video() {
    let url = Url::parse("https://youtu.be/5NV6Rdv1a3I").unwrap();
    let tracks = resolve(&config(Path::new("yt-dlp")), &QueryType::Link(url), LIMIT)
        .await
        .unwrap()
        .tracks;
    assert_eq!(tracks.len(), 1);
    assert_eq!(
        tracks[0].location,
        Location::Youtube("5NV6Rdv1a3I".parse().unwrap())
    );
}

#[tokio::test]
#[ignore = "runs yt-dlp and needs the network"]
async fn real_playlist_gives_its_videos() {
    let url =
        Url::parse("https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI")
            .unwrap();
    let tracks = resolve(&config(Path::new("yt-dlp")), &QueryType::Link(url), LIMIT)
        .await
        .unwrap()
        .tracks;
    assert!(tracks.len() > 1, "{}", tracks.len());
    assert!(tracks.iter().all(|track| !track.title.is_empty()));
}

#[tokio::test]
async fn missing_yt_dlp_is_a_spawn_error() {
    let query = QueryType::Phrase("x".to_owned());
    let result = resolve(&config(Path::new("/nonexistent/yt-dlp")), &query, LIMIT).await;
    assert!(matches!(result, Err(YoutubeError::Spawn(_))), "{result:?}");
}

fn fake_yt_dlp(script: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("yt-dlp");
    let source = dir.path().join("yt-dlp.src");
    fs::write(&source, format!("#!/bin/sh\n{script}\n")).unwrap();
    let copied = std::process::Command::new("cp")
        .arg(&source)
        .arg(&path)
        .status()
        .unwrap();
    assert!(copied.success());
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
    open_config(&config(yt_dlp))
}

fn open_config(config: &YoutubeConfig) -> Box<dyn Read + Send> {
    let id = "5NV6Rdv1a3I".parse().unwrap();
    match open(config, &id, &Stopper::new()).unwrap() {
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
    let result = open(
        &config(Path::new("/nonexistent/yt-dlp")),
        &id,
        &Stopper::new(),
    );
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
    open_config(&config).read_to_end(&mut Vec::new()).unwrap();
    assert!(runs(&dir)[0].contains("youtube:player_client=visionos,tv_samsung"));
}

#[test]
fn empty_player_clients_leave_the_choice_to_yt_dlp() {
    let (dir, path) = fake_yt_dlp(&logged("exit 1"));
    let config = YoutubeConfig {
        player_clients: Vec::new(),
        ..config(&path)
    };
    assert!(open_config(&config).read_to_end(&mut Vec::new()).is_err());
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
    let tracks = resolve(&config(&path), &query, LIMIT).await.unwrap().tracks;
    assert_eq!(tracks, [track("5NV6Rdv1a3I", "Daft Punk - Get Lucky")]);

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
    let result = resolve(&config, &query, LIMIT).await;
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
    let result = resolve(&config(&path), &query, LIMIT).await;
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

fn open_once(yt_dlp: &Path, clients: Clients) -> Box<dyn Read + Send> {
    let id = "5NV6Rdv1a3I".parse().unwrap();
    match open_with(&config(yt_dlp), &id, clients, &Stopper::new()).unwrap() {
        AudioInput::Stream(reader) => reader,
        AudioInput::File(_) => panic!("expected a stream"),
    }
}

#[test]
fn open_with_tries_only_once() {
    let (dir, path) = fake_yt_dlp(&logged("exit 1"));
    let err = open_once(&path, Clients::Preferred)
        .read_to_end(&mut Vec::new())
        .unwrap_err();
    assert!(err.to_string().contains("yt-dlp exited"), "{err}");
    assert_eq!(runs(&dir).len(), 1);
}

#[test]
fn open_with_uses_the_given_clients() {
    let (dir, path) = fake_yt_dlp(&logged("printf x"));
    open_once(&path, Clients::Preferred)
        .read_to_end(&mut Vec::new())
        .unwrap();
    open_once(&path, Clients::Default)
        .read_to_end(&mut Vec::new())
        .unwrap();
    let runs = runs(&dir);
    assert!(runs[0].contains("youtube:player_client=visionos"));
    assert!(!runs[1].contains("--extractor-args"));
}

#[tokio::test]
async fn resolve_with_tries_only_once() {
    let (dir, path) = fake_yt_dlp(&logged("exit 1"));
    let query = QueryType::Phrase("x".to_owned());
    let result = resolve_with(&config(&path), &query, LIMIT, Clients::Preferred).await;
    assert!(
        matches!(result, Err(YoutubeError::Failed { .. })),
        "{result:?}"
    );
    assert_eq!(runs(&dir).len(), 1);
}

#[tokio::test]
async fn resolve_with_default_clients_has_no_extractor_args() {
    let script = format!(
        r#"cat <<'JSON'
{VIDEO_JSON}
JSON"#
    );
    let (dir, path) = fake_yt_dlp(&logged(&script));
    let query = QueryType::Phrase("get lucky".to_owned());
    resolve_with(&config(&path), &query, LIMIT, Clients::Default)
        .await
        .unwrap();
    assert!(!runs(&dir)[0].contains("--extractor-args"));
}

fn playlist_script() -> String {
    let json = search_json(&format!(
        "{},{}",
        entry("5NV6Rdv1a3I", "first"),
        entry("-abc_def-gh", "second")
    ));
    format!(
        r#"cat <<'JSON'
{json}
JSON"#
    )
}

#[tokio::test]
async fn phrase_gives_only_the_first_result() {
    let (_dir, path) = fake_yt_dlp(&playlist_script());
    let query = QueryType::Phrase("daft punk".to_owned());
    let found = resolve(&config(&path), &query, LIMIT).await.unwrap();
    assert_eq!(found.tracks, [track("5NV6Rdv1a3I", "first")]);
    assert_eq!(found.playlist, None);
}

#[tokio::test]
async fn playlist_link_gives_every_video() {
    let (dir, path) = fake_yt_dlp(&logged(&playlist_script()));
    let url =
        Url::parse("https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI")
            .unwrap();
    let found = resolve(&config(&path), &QueryType::Link(url), LIMIT)
        .await
        .unwrap();
    assert_eq!(
        found.tracks,
        [
            track("5NV6Rdv1a3I", "first"),
            track("-abc_def-gh", "second")
        ]
    );
    assert_eq!(found.playlist.unwrap().title, "daft punk");
    assert!(
        runs(&dir)[0].ends_with(
            "-- https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI"
        )
    );
}

#[tokio::test]
async fn playlist_link_asks_only_for_the_limit() {
    let (dir, path) = fake_yt_dlp(&logged(&playlist_script()));
    let url =
        Url::parse("https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI")
            .unwrap();
    let limit = NonZeroUsize::new(25).unwrap();
    resolve(&config(&path), &QueryType::Link(url), limit)
        .await
        .unwrap();
    assert!(runs(&dir)[0].contains("--playlist-items 1:25 --"));
}

#[tokio::test]
async fn video_link_and_phrase_ask_for_no_limit() {
    let (dir, path) = fake_yt_dlp(&logged(&playlist_script()));
    let url = Url::parse("https://www.youtube.com/watch?v=5NV6Rdv1a3I&list=PLxyz").unwrap();
    let found = resolve(&config(&path), &QueryType::Link(url), LIMIT)
        .await
        .unwrap();
    assert_eq!(found.playlist, None);
    resolve(&config(&path), &QueryType::Phrase("x".to_owned()), LIMIT)
        .await
        .unwrap();
    assert!(
        runs(&dir)
            .iter()
            .all(|run| !run.contains("--playlist-items"))
    );
}

#[tokio::test]
async fn lookup_skips_unavailable_videos() {
    let (dir, path) = fake_yt_dlp(&logged(&playlist_script()));
    let query = QueryType::Phrase("x".to_owned());
    resolve(&config(&path), &query, LIMIT).await.unwrap();
    assert!(runs(&dir)[0].contains("--compat-options no-youtube-unavailable-videos"));
}

const AGE_GATE: &str = "echo 'ERROR: [youtube] Tq92D6wQ1mg: Sign in to confirm your age. This video may be inappropriate for some users.' >&2; exit 1";

#[tokio::test]
async fn age_restricted_lookup_is_not_retried() {
    let (dir, path) = fake_yt_dlp(&logged(AGE_GATE));
    let query = QueryType::Phrase("x".to_owned());
    let result = resolve(&config(&path), &query, LIMIT).await;
    assert!(
        matches!(result, Err(YoutubeError::AgeRestricted)),
        "{result:?}"
    );
    assert_eq!(runs(&dir).len(), 1);
}

#[test]
fn age_restricted_stream_is_not_retried() {
    let (dir, path) = fake_yt_dlp(&logged(AGE_GATE));
    let err = open_stream(&path).read_to_end(&mut Vec::new()).unwrap_err();
    assert_eq!(err.to_string(), "this video is age-restricted");
    assert_eq!(runs(&dir).len(), 1);
}

#[tokio::test]
async fn every_age_gate_reason_is_recognised() {
    for reason in [
        "Sign in to confirm your age",
        "This video is age-restricted and YouTube is requiring account age-verification",
        "SIGN IN TO CONFIRM YOUR AGE",
    ] {
        let (_dir, path) = fake_yt_dlp(&format!("echo 'ERROR: {reason}' >&2; exit 1"));
        let query = QueryType::Phrase("x".to_owned());
        let result = resolve_with(&config(&path), &query, LIMIT, Clients::Default).await;
        assert!(
            matches!(result, Err(YoutubeError::AgeRestricted)),
            "{reason}: {result:?}"
        );
    }
}

#[tokio::test]
async fn other_failures_stay_failed() {
    let (_dir, path) =
        fake_yt_dlp("echo 'ERROR: [youtube] x: This video is unavailable' >&2; exit 1");
    let query = QueryType::Phrase("x".to_owned());
    let result = resolve_with(&config(&path), &query, LIMIT, Clients::Default).await;
    assert!(
        matches!(result, Err(YoutubeError::Failed { .. })),
        "{result:?}"
    );
}

#[test]
fn age_restricted_message_is_safe_for_the_chat() {
    assert_eq!(
        YoutubeError::AgeRestricted.to_string(),
        "this video is age-restricted"
    );
}

#[tokio::test]
#[ignore = "runs yt-dlp and needs the network"]
async fn real_age_restricted_video_is_recognised() {
    let url = Url::parse("https://www.youtube.com/watch?v=Tq92D6wQ1mg").unwrap();
    let result = resolve(&config(Path::new("yt-dlp")), &QueryType::Link(url), LIMIT).await;
    assert!(
        matches!(result, Err(YoutubeError::AgeRestricted)),
        "{result:?}"
    );
}

#[test]
fn a_stopped_stream_ends_at_once_and_is_not_retried() {
    let (dir, yt_dlp) = fake_yt_dlp(
        r#"echo run >> "$(dirname "$0")/runs"
exec sleep 30"#,
    );
    let stop = Stopper::new();
    let id = "5NV6Rdv1a3I".parse().unwrap();
    let AudioInput::Stream(mut reader) = open(&config(&yt_dlp), &id, &stop).unwrap() else {
        panic!("expected a stream");
    };
    let reading = std::thread::spawn(move || {
        let mut buf = [0u8; 16];
        reader.read(&mut buf)
    });
    std::thread::sleep(Duration::from_millis(200));
    let start = Instant::now();
    stop.stop();
    let read = reading.join().unwrap();
    assert_eq!(read.unwrap(), 0);
    assert!(start.elapsed() < Duration::from_secs(2));
    let runs = fs::read_to_string(dir.path().join("runs")).unwrap();
    assert_eq!(runs.lines().count(), 1, "{runs}");
}

const COOKIES: &str = "# Netscape HTTP Cookie File\n";

fn cookie_file(dir: &TempDir, mode: u32) -> PathBuf {
    let path = dir.path().join("youtube-cookies.txt");
    fs::write(&path, COOKIES).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    path
}

fn with_cookies(yt_dlp: &Path, cookies: &Path) -> YoutubeConfig {
    YoutubeConfig {
        cookies_file: Some(cookies.to_path_buf()),
        ..config(yt_dlp)
    }
}

fn reading_cookies(body: &str) -> String {
    logged(&format!(
        r#"c=; p=; for a; do [ "$p" = --cookies ] && c=$a; p=$a; done
if [ -n "$c" ]; then echo "$c" >> "$d/copies"; cat "$c" >> "$d/seen"; echo rewritten > "$c"; fi
{body}"#
    ))
}

fn copies(dir: &TempDir) -> Vec<PathBuf> {
    fs::read_to_string(dir.path().join("copies"))
        .unwrap_or_default()
        .lines()
        .map(PathBuf::from)
        .collect()
}

fn video_script() -> String {
    format!(
        r#"cat <<'JSON'
{VIDEO_JSON}
JSON"#
    )
}

#[test]
fn cookies_go_only_to_runs_with_the_default_clients() {
    let (dir, path) = fake_yt_dlp(&reading_cookies(
        r#"[ "$run" -ge 2 ] || exit 1; printf audio"#,
    ));
    let cookies = cookie_file(&dir, 0o600);
    open_config(&with_cookies(&path, &cookies))
        .read_to_end(&mut Vec::new())
        .unwrap();

    let runs = runs(&dir);
    assert_eq!(runs.len(), 2);
    assert!(!runs[0].contains("--cookies"));
    assert!(runs[1].contains("--cookies"));
    assert_eq!(
        fs::read_to_string(dir.path().join("seen")).unwrap(),
        COOKIES
    );
}

#[tokio::test]
async fn without_player_clients_the_first_lookup_has_cookies() {
    let (dir, path) = fake_yt_dlp(&reading_cookies(&video_script()));
    let cookies = cookie_file(&dir, 0o600);
    let config = YoutubeConfig {
        player_clients: Vec::new(),
        ..with_cookies(&path, &cookies)
    };
    let query = QueryType::Phrase("get lucky".to_owned());
    resolve(&config, &query, LIMIT).await.unwrap();
    assert!(runs(&dir)[0].contains("--cookies"));
}

#[tokio::test]
async fn the_copy_is_removed_after_the_lookup() {
    let (dir, path) = fake_yt_dlp(&reading_cookies(&video_script()));
    let cookies = cookie_file(&dir, 0o600);
    let query = QueryType::Phrase("get lucky".to_owned());
    resolve_with(
        &with_cookies(&path, &cookies),
        &query,
        LIMIT,
        Clients::Default,
    )
    .await
    .unwrap();

    let copies = copies(&dir);
    assert_eq!(copies.len(), 1);
    assert_ne!(copies[0], cookies);
    assert!(!copies[0].exists());
    assert_eq!(fs::read_to_string(&cookies).unwrap(), COOKIES);
}

#[test]
fn the_copy_is_removed_after_the_stream() {
    let (dir, path) = fake_yt_dlp(&reading_cookies("printf audio"));
    let cookies = cookie_file(&dir, 0o600);
    let id = "5NV6Rdv1a3I".parse().unwrap();
    let AudioInput::Stream(mut reader) = open_with(
        &with_cookies(&path, &cookies),
        &id,
        Clients::Default,
        &Stopper::new(),
    )
    .unwrap() else {
        panic!("expected a stream");
    };
    reader.read_to_end(&mut Vec::new()).unwrap();
    drop(reader);

    let copies = copies(&dir);
    assert_eq!(copies.len(), 1);
    assert!(!copies[0].exists());
    assert_eq!(fs::read_to_string(&cookies).unwrap(), COOKIES);
}

#[tokio::test]
async fn an_unprotected_cookie_file_is_left_out() {
    let (dir, path) = fake_yt_dlp(&reading_cookies(&video_script()));
    let cookies = cookie_file(&dir, 0o644);
    let query = QueryType::Phrase("get lucky".to_owned());
    resolve_with(
        &with_cookies(&path, &cookies),
        &query,
        LIMIT,
        Clients::Default,
    )
    .await
    .unwrap();
    assert!(!runs(&dir)[0].contains("--cookies"));
}

#[tokio::test]
async fn without_a_cookie_file_no_cookies_are_passed() {
    let (dir, path) = fake_yt_dlp(&reading_cookies(&video_script()));
    let query = QueryType::Phrase("get lucky".to_owned());
    resolve_with(&config(&path), &query, LIMIT, Clients::Default)
        .await
        .unwrap();
    assert!(!runs(&dir)[0].contains("--cookies"));
}

#[test]
fn a_start_that_hangs_is_stopped_and_retried() {
    let (dir, path) = fake_yt_dlp(&logged(
        r#"[ "$run" -ge 2 ] || exec sleep 30; printf audio"#,
    ));
    let start = Instant::now();
    let mut data = Vec::new();
    open_stream(&path).read_to_end(&mut data).unwrap();
    assert_eq!(data, b"audio");
    assert_eq!(runs(&dir).len(), 2);
    assert!(start.elapsed() >= Duration::from_millis(500));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_start_that_hangs_without_retries_fails() {
    let (dir, path) = fake_yt_dlp(&logged("exec sleep 30"));
    let err = open_once(&path, Clients::Default)
        .read_to_end(&mut Vec::new())
        .unwrap_err();
    assert!(err.to_string().contains("yt-dlp exited"), "{err}");
    assert_eq!(runs(&dir).len(), 1);
}

#[test]
fn a_slow_start_within_the_limit_is_kept() {
    let (dir, path) = fake_yt_dlp(&logged("sleep 0.2; printf audio"));
    let mut data = Vec::new();
    open_stream(&path).read_to_end(&mut data).unwrap();
    assert_eq!(data, b"audio");
    assert_eq!(runs(&dir).len(), 1);
}

#[test]
fn a_pause_after_the_first_byte_is_not_a_hung_start() {
    let (dir, path) = fake_yt_dlp(&logged("printf a; sleep 1; printf b"));
    let mut data = Vec::new();
    open_stream(&path).read_to_end(&mut data).unwrap();
    assert_eq!(data, b"ab");
    assert_eq!(runs(&dir).len(), 1);
}
