use std::fs;

use super::harness::{Harness, on_channel};
use crate::player::state::Ended;

#[tokio::test]
async fn add_when_idle_starts_playing() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    assert_eq!(h.playing(), Some("a"));
    assert!(h.queued().is_empty());
}

#[tokio::test]
async fn add_while_playing_only_queues() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    assert_eq!(h.playing(), Some("a"));
    assert_eq!(h.queued(), ["b", "c"]);
}

#[tokio::test]
async fn add_while_paused_stays_paused() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    h.send("!song pause").await;
    h.add(&["b.mp3"]).await;
    assert_eq!(h.paused(), Some("a"));
    assert!(h.control().paused);
    assert_eq!(h.queued(), ["b"]);
}

#[tokio::test]
async fn add_missing_file_is_an_error_and_changes_nothing() {
    let mut h = Harness::new();
    assert_eq!(
        h.add_outbox("!song add nope.mp3").await,
        on_channel("no such file in the music directory")
    );
    assert!(h.is_idle());
    assert!(h.queued().is_empty());
}

#[tokio::test]
async fn finished_starts_the_next_track_and_keeps_the_buffered_end() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let first = h.current_stop();
    h.finish_current();
    assert_eq!(h.playing(), Some("b"));
    assert!(!first.is_cancelled());
    assert_eq!(h.control().min_stream_id, 0);
}

#[tokio::test]
async fn finished_with_an_empty_queue_goes_idle() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    h.finish_current();
    assert!(h.is_idle());
    assert_eq!(h.control().min_stream_id, 0);
}

#[tokio::test]
async fn stale_finished_is_ignored() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    let old = h.current_id();
    h.send("!song next").await;
    h.player.finished(Ended {
        id: old,
        failure: None,
    });
    assert_eq!(h.playing(), Some("b"));
    assert_eq!(h.queued(), ["c"]);
}

#[tokio::test]
async fn unopenable_track_is_skipped() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    fs::remove_file(h.dir.path().join("b.mp3")).unwrap();
    h.finish_current();
    assert_eq!(h.playing(), Some("c"));
}

#[tokio::test]
async fn ids_only_go_up() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let first = h.current_id();
    h.send("!song next").await;
    assert!(h.current_id().0 > first.0);
}

#[tokio::test]
async fn pause_holds_the_frames() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    assert_eq!(h.send("!song pause").await, None);
    assert_eq!(h.paused(), Some("a"));
    assert!(h.control().paused);
    assert!(!h.current_stop().is_cancelled());
}

#[tokio::test]
async fn pause_when_idle_does_nothing() {
    let mut h = Harness::new();
    assert_eq!(h.send("!song pause").await, None);
    assert!(h.is_idle());
    assert!(!h.control().paused);
}

#[tokio::test]
async fn resume_continues_the_paused_track() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    let id = h.current_id();
    h.send("!song pause").await;
    assert_eq!(h.send("!song resume").await, None);
    assert_eq!(h.playing(), Some("a"));
    assert_eq!(h.current_id(), id);
    assert!(!h.control().paused);
}

#[tokio::test]
async fn resume_when_idle_with_an_empty_queue_is_an_error() {
    let mut h = Harness::new();
    assert_eq!(
        h.send("!song resume").await,
        Some("the queue is empty".to_string())
    );
    assert!(h.is_idle());
}

#[tokio::test]
async fn finished_while_paused_waits_idle_and_stays_paused() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.send("!song pause").await;
    h.finish_current();
    assert!(h.is_idle());
    assert!(h.control().paused);
    assert_eq!(h.queued(), ["b"]);
}

#[tokio::test]
async fn resume_after_finishing_while_paused_starts_the_queue() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.send("!song pause").await;
    h.finish_current();
    assert_eq!(h.send("!song resume").await, None);
    assert_eq!(h.playing(), Some("b"));
    assert!(!h.control().paused);
    assert_eq!(h.control().min_stream_id, 0);
}

#[tokio::test]
async fn add_after_finishing_while_paused_is_heard() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    h.send("!song pause").await;
    h.finish_current();
    h.add(&["b.mp3"]).await;
    assert_eq!(h.playing(), Some("b"));
    assert!(!h.control().paused);
}

#[tokio::test]
async fn next_cancels_the_track_and_drops_its_frames() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let old_id = h.current_id();
    let old_stop = h.current_stop();
    assert_eq!(h.send("!song next").await, None);
    assert_eq!(h.playing(), Some("b"));
    assert!(old_stop.is_cancelled());
    assert!(h.control().min_stream_id > old_id.0);
    assert!(h.control().min_stream_id <= h.current_id().0);
}

#[tokio::test]
async fn next_with_an_empty_queue_stops() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    let old_id = h.current_id();
    let old_stop = h.current_stop();
    assert_eq!(h.send("!song next").await, None);
    assert!(h.is_idle());
    assert!(old_stop.is_cancelled());
    assert!(h.control().min_stream_id > old_id.0);
}

#[tokio::test]
async fn next_while_paused_unpauses_for_the_next_track() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.send("!song pause").await;
    h.send("!song next").await;
    assert_eq!(h.playing(), Some("b"));
    assert!(!h.control().paused);
}
