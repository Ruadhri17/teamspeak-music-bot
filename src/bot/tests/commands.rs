use super::harness::{Harness, author, on_channel, reply};
use crate::{
    chat::MAX_MESSAGE_LEN,
    config::Volume,
    permissions::Role,
    teamspeak::{ReplyTo, TeamspeakRequest},
};

#[tokio::test]
async fn current_in_every_state() {
    let mut h = Harness::new();
    assert_eq!(h.send("!song current"), reply("nothing is playing"));
    h.add(&["a.mp3"]).await;
    assert_eq!(h.send("!song current"), reply("a"));
    h.send("!song pause");
    assert_eq!(h.send("!song current"), reply("a (paused)"));
}

#[tokio::test]
async fn queue_show_when_idle_and_empty() {
    let mut h = Harness::new();
    assert_eq!(h.send("!queue show"), reply("the queue is empty"));
}

#[tokio::test]
async fn queue_show_with_the_current_track_only() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    assert_eq!(h.send("!queue show"), reply("Now: a\nthe queue is empty"));
}

#[tokio::test]
async fn queue_show_lists_numbered_tracks() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    assert_eq!(h.send("!queue show"), reply("Now: a\n1. b\n2. c"));
}

#[tokio::test]
async fn queue_show_pages() {
    let mut h = Harness::with_page_size(1);
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    assert_eq!(h.send("!queue show"), reply("Now: a\n1. b\npage 1/2"));
    assert_eq!(h.send("!queue show 2"), reply("Now: a\n2. c\npage 2/2"));
    assert_eq!(
        h.send("!queue show 3"),
        reply("no such page, the queue is shorter")
    );
}

#[tokio::test]
async fn queue_remove_counts_from_one_and_is_silent() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    assert_eq!(h.send("!queue remove 1"), None);
    assert_eq!(h.queued(), ["c"]);
    assert_eq!(h.playing(), Some("a"));
}

#[tokio::test]
async fn queue_remove_last_removes_the_end_and_is_silent() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    assert_eq!(h.send("!queue remove last"), None);
    assert_eq!(h.queued(), ["b"]);
    assert_eq!(h.playing(), Some("a"));
}

#[tokio::test]
async fn queue_remove_last_leaves_the_current_song() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    assert_eq!(h.send("!queue remove last"), reply("the queue is empty"));
    assert_eq!(h.playing(), Some("a"));
}

#[tokio::test]
async fn queue_remove_past_the_end_is_an_error() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    assert_eq!(
        h.send("!queue remove 2"),
        reply("no song at position 2 in the queue")
    );
    assert_eq!(h.queued(), ["b"]);
}

#[tokio::test]
async fn queue_clear_stops_everything() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let id = h.current_id();
    let stop = h.current_stop();
    assert_eq!(h.send("!queue clear"), None);
    assert!(h.is_idle());
    assert!(h.queued().is_empty());
    assert!(stop.is_stopped());
    assert!(h.control().min_stream_id > id.0);
}

#[tokio::test]
async fn ping_answers_pong() {
    let mut h = Harness::new();
    assert_eq!(h.send("!bot ping"), reply("pong"));
}

#[tokio::test]
async fn config_commands_say_they_are_not_supported_yet() {
    let mut h = Harness::new();
    let admin = author(1, Role::Admin, true);
    for text in ["!bot config list", "!bot config reload"] {
        assert_eq!(
            h.send_as(ReplyTo::Channel, &admin, text),
            reply("!bot config is not supported yet"),
            "{text}"
        );
    }
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn summon_is_passed_to_the_client_with_the_sender() {
    let mut h = Harness::new();
    let sender = ReplyTo::Client(7);
    assert_eq!(h.send_from(sender, "!bot summon"), None);
    assert_eq!(
        h.teamspeak_requests(),
        [TeamspeakRequest::Summon {
            sender_id: 7,
            reply_to: sender,
            admin: false,
        }]
    );
    assert!(h.is_idle());
}

#[tokio::test]
async fn summon_by_an_admin_is_marked() {
    let mut h = Harness::new();
    let admin = author(7, Role::Admin, false);
    assert_eq!(h.send_as(ReplyTo::Client(7), &admin, "!bot summon"), None);
    assert_eq!(
        h.teamspeak_requests(),
        [TeamspeakRequest::Summon {
            sender_id: 7,
            reply_to: ReplyTo::Client(7),
            admin: true,
        }]
    );
}

#[tokio::test]
async fn unsupported_source_replies_and_keeps_idle() {
    let mut h = Harness::new();
    for source in ["tidal", "spotify", "soundcloud"] {
        assert_eq!(
            h.add_outbox(&format!("!song add --source {source} x"))
                .await,
            on_channel("this source is not supported yet")
        );
        assert!(h.is_idle());
    }
}

#[tokio::test]
async fn youtube_without_yt_dlp_replies_and_keeps_idle() {
    let mut h = Harness::new();
    assert_eq!(
        h.add_outbox("!song add --source youtube daft punk").await,
        on_channel("yt-dlp could not be started")
    );
    assert!(h.is_idle());
}

#[tokio::test]
async fn help_is_sent_to_the_chat() {
    let mut h = Harness::new();
    let help = h.send("!song help").unwrap();
    assert!(
        help.starts_with("Available commands:\n!song add "),
        "{help}"
    );
    assert!(h.is_idle());
}

#[tokio::test]
async fn titles_cannot_format_the_chat() {
    let mut h = Harness::new();
    std::fs::write(h.dir.path().join("[url=https]x[b].mp3"), b"audio").unwrap();
    h.add(&["a.mp3", "[url=https]x[b].mp3"]).await;
    assert_eq!(
        h.send("!queue show"),
        reply("Now: a\n1. \\[url=https\\]x\\[b\\]")
    );
    h.track_heard();
    assert_eq!(h.send("!song current"), reply("\\[url=https\\]x\\[b\\]"));
}

#[tokio::test]
async fn volume_starts_at_the_default_and_is_shown() {
    let mut h = Harness::new();
    assert_eq!(h.control().volume, Volume::new(50).unwrap());
    assert_eq!(h.send("!bot volume"), reply("volume: 50"));
}

#[tokio::test]
async fn setting_the_volume_is_silent_and_reaches_the_client() {
    let mut h = Harness::new();
    assert_eq!(h.send("!bot volume 30"), None);
    assert_eq!(h.control().volume, Volume::new(30).unwrap());
    assert_eq!(h.send("!bot volume"), reply("volume: 30"));
    assert_eq!(h.send("!bot volume 0"), None);
    assert_eq!(h.control().volume, Volume::new(0).unwrap());
}

#[tokio::test]
async fn volume_above_the_maximum_is_refused() {
    let mut h = Harness::with_config(|config| {
        config.audio.default_volume = Volume::new(20).unwrap();
        config.audio.max_volume = Volume::new(60).unwrap();
    });
    assert_eq!(h.send("!bot volume 61"), reply("the highest volume is 60"));
    assert_eq!(h.control().volume, Volume::new(20).unwrap());
    assert_eq!(h.send("!bot volume 60"), None);
    assert_eq!(h.control().volume, Volume::new(60).unwrap());
}

#[tokio::test]
async fn volume_does_not_touch_playback() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    let stream = h.current_id();
    assert_eq!(h.send("!bot volume 10"), None);
    assert_eq!(h.playing(), Some("a"));
    assert_eq!(h.current_id(), stream);
    assert!(!h.control().paused);
}

#[tokio::test]
async fn queue_show_fits_long_titles_into_one_message() {
    let mut h = Harness::new();
    let names: Vec<String> = (0..17)
        .map(|n| format!("{n:02} {}", "Very Long Title [Official Video] ".repeat(4)))
        .collect();
    for name in &names {
        std::fs::write(h.dir.path().join(format!("{name}.mp3")), b"audio").unwrap();
    }
    let files: Vec<String> = names.iter().map(|name| format!("{name}.mp3")).collect();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    h.add(&files).await;
    let text = h.send("!queue show").unwrap();
    assert!(text.len() <= crate::chat::MAX_MESSAGE_LEN, "{}", text.len());
    assert!(text.starts_with("Now: 00 Very Long"), "{text}");
    assert!(text.contains("15. 15 Very Long"), "{text}");
    assert!(text.ends_with("page 1/2"), "{text}");
    assert!(text.lines().all(|line| line.chars().count() < 70), "{text}");
}

#[tokio::test]
async fn album_and_playlist_search_say_they_are_not_supported_yet() {
    let mut h = Harness::new();
    for text in [
        "!song add --album abba gold",
        "!song add --playlist summer hits",
    ] {
        assert_eq!(
            h.send(text),
            reply(
                "searching for albums and playlists is not supported yet, add a playlist link instead"
            ),
            "{text}"
        );
    }
    assert!(h.is_idle());
    assert!(h.bot.lookups.is_empty());
}

async fn full_page_of(h: &mut Harness, title: impl Fn(usize) -> String) {
    let names: Vec<String> = (0..16).map(|i| format!("{}.mp3", title(i))).collect();
    for name in &names {
        std::fs::write(h.dir.path().join(name), b"audio").unwrap();
    }
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    h.add(&names).await;
}

fn assert_whole_page(text: &str) {
    assert!(text.len() <= MAX_MESSAGE_LEN, "{} bytes", text.len());
    assert!(text.lines().any(|line| line.starts_with("15. ")), "{text}");
    assert!(!text.lines().any(|line| line == "…"), "{text}");
}

#[tokio::test]
async fn a_page_of_long_korean_titles_shows_every_entry() {
    let mut h = Harness::new();
    full_page_of(&mut h, |i| format!("{}{i:02}", "노래".repeat(24))).await;
    let text = h.send("!queue show").unwrap();
    assert_whole_page(&text);
    assert_eq!(
        h.send("!queue show 2"),
        reply("no such page, the queue is shorter")
    );
}

#[tokio::test]
async fn a_page_of_emoji_titles_shows_every_entry() {
    let mut h = Harness::new();
    full_page_of(&mut h, |i| format!("{}{i:02}", "🎵".repeat(48))).await;
    assert_whole_page(&h.send("!queue show").unwrap());
}

#[tokio::test]
async fn escaped_brackets_count_toward_the_page() {
    let mut h = Harness::new();
    full_page_of(&mut h, |i| format!("{}{i:02}", "[b]".repeat(16))).await;
    let text = h.send("!queue show").unwrap();
    assert_whole_page(&text);
    assert!(text.contains("\\[b\\]"), "{text}");
}

#[tokio::test]
async fn short_titles_are_not_cut() {
    let mut h = Harness::new();
    full_page_of(&mut h, |i| format!("Dancing Queen {i:02}")).await;
    let text = h.send("!queue show").unwrap();
    assert_whole_page(&text);
    assert!(text.contains("15. Dancing Queen 15"), "{text}");
}

#[tokio::test]
async fn the_largest_page_of_long_titles_fits() {
    let mut h = Harness::with_page_size(30);
    let names: Vec<String> = (0..31)
        .map(|i| format!("{}{i:02}.mp3", "가".repeat(48)))
        .collect();
    for name in &names {
        std::fs::write(h.dir.path().join(name), b"audio").unwrap();
    }
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    h.add(&names).await;
    let text = h.send("!queue show").unwrap();
    assert!(text.len() <= MAX_MESSAGE_LEN, "{} bytes", text.len());
    assert!(text.lines().any(|line| line.starts_with("30. ")), "{text}");
}
