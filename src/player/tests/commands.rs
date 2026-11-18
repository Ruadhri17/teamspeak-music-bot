use super::harness::Harness;

fn reply(text: &str) -> Option<String> {
    Some(text.to_string())
}

#[test]
fn current_in_every_state() {
    let mut h = Harness::new();
    assert_eq!(h.send("!song current"), reply("nothing is playing"));
    h.add(&["a.mp3"]);
    assert_eq!(h.send("!song current"), reply("a"));
    h.send("!song pause");
    assert_eq!(h.send("!song current"), reply("a (paused)"));
}

#[test]
fn queue_show_when_idle_and_empty() {
    let mut h = Harness::new();
    assert_eq!(h.send("!queue show"), reply("the queue is empty"));
}

#[test]
fn queue_show_with_the_current_track_only() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]);
    assert_eq!(h.send("!queue show"), reply("Now: a\nthe queue is empty"));
}

#[test]
fn queue_show_lists_numbered_tracks() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]);
    assert_eq!(h.send("!queue show"), reply("Now: a\n1. b\n2. c"));
}

#[test]
fn queue_show_pages() {
    let mut h = Harness::with_page_size(1);
    h.add(&["a.mp3", "b.mp3", "c.mp3"]);
    assert_eq!(h.send("!queue show"), reply("Now: a\n1. b\npage 1/2"));
    assert_eq!(h.send("!queue show 2"), reply("Now: a\n2. c\npage 2/2"));
    assert_eq!(
        h.send("!queue show 3"),
        reply("no such page, the queue is shorter")
    );
}

#[test]
fn queue_remove_counts_from_one_and_is_silent() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]);
    assert_eq!(h.send("!queue remove 1"), None);
    assert_eq!(h.queued(), ["c"]);
    assert_eq!(h.playing(), Some("a"));
}

#[test]
fn queue_remove_past_the_end_is_an_error() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]);
    assert_eq!(
        h.send("!queue remove 2"),
        reply("no song at position 2 in the queue")
    );
    assert_eq!(h.queued(), ["b"]);
}

#[test]
fn queue_clear_stops_everything() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]);
    let id = h.current_id();
    let stop = h.current_stop();
    assert_eq!(h.send("!queue clear"), None);
    assert!(h.is_idle());
    assert!(h.queued().is_empty());
    assert!(stop.is_cancelled());
    assert!(h.control().drop_before > id.0);
}

#[test]
fn ping_answers_pong() {
    let mut h = Harness::new();
    assert_eq!(h.send("!bot ping"), reply("pong"));
}

#[test]
fn bot_commands_are_not_handled_by_the_player() {
    let mut h = Harness::new();
    assert_eq!(h.send("!bot summon"), None);
    assert_eq!(h.send("!bot config list"), None);
    assert_eq!(h.send("!bot config reload"), None);
}

#[test]
fn unsupported_source_replies_and_keeps_idle() {
    let mut h = Harness::new();
    for source in ["tidal", "spotify", "youtube", "soundcloud"] {
        assert_eq!(
            h.send(&format!("!song add --source {source} x")),
            reply("this source is not supported yet")
        );
        assert!(h.is_idle());
    }
}
