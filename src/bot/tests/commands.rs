use super::harness::{Harness, author, on_channel};
use crate::{
    config::Volume,
    permissions::Role,
    teamspeak::{ReplyTo, TeamspeakRequest},
};

fn reply(text: &str) -> Option<String> {
    Some(text.to_string())
}

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
async fn config_commands_are_not_handled_by_the_player() {
    let mut h = Harness::new();
    let admin = author(1, Role::Admin, true);
    assert_eq!(
        h.send_as(ReplyTo::Channel, &admin, "!bot config list"),
        None
    );
    assert_eq!(
        h.send_as(ReplyTo::Channel, &admin, "!bot config reload"),
        None
    );
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
    h.finish_current();
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
