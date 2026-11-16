use std::fs;

use super::harness::Harness;

#[test]
fn add_when_idle_starts_playing() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]);
    assert_eq!(h.playing(), Some("a"));
    assert!(h.queued().is_empty());
}

#[test]
fn add_while_playing_only_queues() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]);
    assert_eq!(h.playing(), Some("a"));
    assert_eq!(h.queued(), ["b", "c"]);
}

#[test]
fn add_while_paused_stays_paused() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]);
    h.send("!song pause");
    h.add(&["b.mp3"]);
    assert_eq!(h.paused(), Some("a"));
    assert!(h.control().paused);
    assert_eq!(h.queued(), ["b"]);
}

#[test]
fn add_missing_file_is_an_error_and_changes_nothing() {
    let mut h = Harness::new();
    assert_eq!(
        h.send("!song add nope.mp3"),
        Some("no such file in the music directory".to_string())
    );
    assert!(h.is_idle());
    assert!(h.queued().is_empty());
}

#[test]
fn finished_starts_the_next_track_and_keeps_the_buffered_end() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]);
    let first = h.current_stop();
    h.finish_current();
    assert_eq!(h.playing(), Some("b"));
    assert!(!first.is_cancelled());
    assert_eq!(h.control().drop_before, 0);
}

#[test]
fn finished_with_an_empty_queue_goes_idle() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]);
    h.finish_current();
    assert!(h.is_idle());
    assert_eq!(h.control().drop_before, 0);
}

#[test]
fn stale_finished_is_ignored() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]);
    let old = h.current_id();
    h.send("!song next");
    h.player.finished(old);
    assert_eq!(h.playing(), Some("b"));
    assert_eq!(h.queued(), ["c"]);
}

#[test]
fn unopenable_track_is_skipped() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]);
    fs::remove_file(h.dir.path().join("b.mp3")).unwrap();
    h.finish_current();
    assert_eq!(h.playing(), Some("c"));
}

#[test]
fn ids_only_go_up() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]);
    let first = h.current_id();
    h.send("!song next");
    assert!(h.current_id().0 > first.0);
}

#[test]
fn pause_holds_the_frames() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]);
    assert_eq!(h.send("!song pause"), None);
    assert_eq!(h.paused(), Some("a"));
    assert!(h.control().paused);
    assert!(!h.current_stop().is_cancelled());
}

#[test]
fn pause_when_idle_does_nothing() {
    let mut h = Harness::new();
    assert_eq!(h.send("!song pause"), None);
    assert!(h.is_idle());
    assert!(!h.control().paused);
}

#[test]
fn resume_continues_the_paused_track() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]);
    let id = h.current_id();
    h.send("!song pause");
    assert_eq!(h.send("!song resume"), None);
    assert_eq!(h.playing(), Some("a"));
    assert_eq!(h.current_id(), id);
    assert!(!h.control().paused);
}

#[test]
fn resume_when_idle_with_an_empty_queue_is_an_error() {
    let mut h = Harness::new();
    assert_eq!(
        h.send("!song resume"),
        Some("the queue is empty".to_string())
    );
    assert!(h.is_idle());
}

#[test]
fn finished_while_paused_waits_idle_and_stays_paused() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]);
    h.send("!song pause");
    h.finish_current();
    assert!(h.is_idle());
    assert!(h.control().paused);
    assert_eq!(h.queued(), ["b"]);
}

#[test]
fn resume_after_finishing_while_paused_starts_the_queue() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]);
    h.send("!song pause");
    h.finish_current();
    assert_eq!(h.send("!song resume"), None);
    assert_eq!(h.playing(), Some("b"));
    assert!(!h.control().paused);
    assert_eq!(h.control().drop_before, 0);
}

#[test]
fn add_after_finishing_while_paused_is_heard() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]);
    h.send("!song pause");
    h.finish_current();
    h.add(&["b.mp3"]);
    assert_eq!(h.playing(), Some("b"));
    assert!(!h.control().paused);
}

#[test]
fn next_cancels_the_track_and_drops_its_frames() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]);
    let old_id = h.current_id();
    let old_stop = h.current_stop();
    assert_eq!(h.send("!song next"), None);
    assert_eq!(h.playing(), Some("b"));
    assert!(old_stop.is_cancelled());
    assert!(h.control().drop_before > old_id.0);
    assert!(h.control().drop_before <= h.current_id().0);
}

#[test]
fn next_with_an_empty_queue_stops() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]);
    let old_id = h.current_id();
    let old_stop = h.current_stop();
    assert_eq!(h.send("!song next"), None);
    assert!(h.is_idle());
    assert!(old_stop.is_cancelled());
    assert!(h.control().drop_before > old_id.0);
}

#[test]
fn next_while_paused_unpauses_for_the_next_track() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]);
    h.send("!song pause");
    h.send("!song next");
    assert_eq!(h.playing(), Some("b"));
    assert!(!h.control().paused);
}
