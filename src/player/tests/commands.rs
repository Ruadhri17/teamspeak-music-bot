use super::harness::Harness;

fn reply(text: &str) -> Option<String> {
    Some(text.to_string())
}

#[tokio::test]
async fn current_in_every_state() {
    let mut h = Harness::new();
    assert_eq!(h.send("!song current").await, reply("nothing is playing"));
    h.add(&["a.mp3"]).await;
    assert_eq!(h.send("!song current").await, reply("a"));
    h.send("!song pause").await;
    assert_eq!(h.send("!song current").await, reply("a (paused)"));
}

#[tokio::test]
async fn queue_show_when_idle_and_empty() {
    let mut h = Harness::new();
    assert_eq!(h.send("!queue show").await, reply("the queue is empty"));
}

#[tokio::test]
async fn queue_show_with_the_current_track_only() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    assert_eq!(
        h.send("!queue show").await,
        reply("Now: a\nthe queue is empty")
    );
}

#[tokio::test]
async fn queue_show_lists_numbered_tracks() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    assert_eq!(h.send("!queue show").await, reply("Now: a\n1. b\n2. c"));
}

#[tokio::test]
async fn queue_show_pages() {
    let mut h = Harness::with_page_size(1);
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    assert_eq!(h.send("!queue show").await, reply("Now: a\n1. b\npage 1/2"));
    assert_eq!(
        h.send("!queue show 2").await,
        reply("Now: a\n2. c\npage 2/2")
    );
    assert_eq!(
        h.send("!queue show 3").await,
        reply("no such page, the queue is shorter")
    );
}

#[tokio::test]
async fn queue_remove_counts_from_one_and_is_silent() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    assert_eq!(h.send("!queue remove 1").await, None);
    assert_eq!(h.queued(), ["c"]);
    assert_eq!(h.playing(), Some("a"));
}

#[tokio::test]
async fn queue_remove_past_the_end_is_an_error() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    assert_eq!(
        h.send("!queue remove 2").await,
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
    assert_eq!(h.send("!queue clear").await, None);
    assert!(h.is_idle());
    assert!(h.queued().is_empty());
    assert!(stop.is_cancelled());
    assert!(h.control().drop_before > id.0);
}

#[tokio::test]
async fn ping_answers_pong() {
    let mut h = Harness::new();
    assert_eq!(h.send("!bot ping").await, reply("pong"));
}

#[tokio::test]
async fn bot_commands_are_not_handled_by_the_player() {
    let mut h = Harness::new();
    assert_eq!(h.send("!bot summon").await, None);
    assert_eq!(h.send("!bot config list").await, None);
    assert_eq!(h.send("!bot config reload").await, None);
}

#[tokio::test]
async fn unsupported_source_replies_and_keeps_idle() {
    let mut h = Harness::new();
    for source in ["tidal", "spotify", "soundcloud"] {
        assert_eq!(
            h.send(&format!("!song add --source {source} x")).await,
            reply("this source is not supported yet")
        );
        assert!(h.is_idle());
    }
}

#[tokio::test]
async fn youtube_without_yt_dlp_replies_and_keeps_idle() {
    let mut h = Harness::new();
    assert_eq!(
        h.send("!song add --source youtube daft punk").await,
        reply("yt-dlp could not be started")
    );
    assert!(h.is_idle());
}

#[tokio::test]
async fn help_is_sent_to_the_chat() {
    let mut h = Harness::new();
    let help = h.send("!song help").await.unwrap();
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
        h.send("!queue show").await,
        reply("Now: a\n1. \\[url=https\\]x\\[b\\]")
    );
    h.finish_current();
    assert_eq!(
        h.send("!song current").await,
        reply("\\[url=https\\]x\\[b\\]")
    );
}
