use std::fs;

use super::harness::{Harness, on_channel, reply};
use crate::player::Ended;

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
    h.send("!song pause");
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
    assert!(!first.is_stopped());
    assert_eq!(h.control().min_stream_id, 0);
}

#[tokio::test]
async fn finished_with_an_empty_queue_goes_idle_once_heard() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    let id = h.current_id();
    h.finish_current();
    assert!(!h.is_idle());
    assert_eq!(h.send("!song current"), reply("a"));
    h.played(id);
    assert!(h.is_idle());
    assert_eq!(h.send("!song current"), reply("nothing is playing"));
    assert_eq!(h.control().min_stream_id, 0);
}

#[tokio::test]
async fn current_is_the_heard_track_until_its_end_went_out() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    let a = h.current_id();
    h.finish_current();
    assert_eq!(h.playing(), Some("b"));
    assert_eq!(h.send("!song current"), reply("a"));
    assert_eq!(h.send("!queue show"), reply("Now: a\nNext: b\n1. c"));
    h.played(a);
    assert_eq!(h.send("!song current"), reply("b"));
    assert_eq!(h.send("!queue show"), reply("Now: b\n1. c"));
}

#[tokio::test]
async fn next_during_the_end_of_a_track_skips_only_that_end() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    h.finish_current();
    let b = h.current_id();
    assert_eq!(h.send("!song next"), None);
    assert_eq!(h.current_id(), b);
    assert_eq!(h.control().min_stream_id, b.0);
    assert_eq!(h.send("!song current"), reply("b"));
    assert_eq!(h.queued(), ["c"]);
}

#[tokio::test]
async fn next_after_the_last_decoded_track_stops() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    h.finish_current();
    assert_eq!(h.send("!song next"), None);
    assert!(h.is_idle());
    assert_eq!(h.send("!song current"), reply("nothing is playing"));
}

#[tokio::test]
async fn short_tracks_are_heard_in_order() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    let a = h.current_id();
    h.finish_current();
    let b = h.current_id();
    h.finish_current();
    assert_eq!(h.playing(), Some("c"));
    assert_eq!(h.send("!song current"), reply("a"));
    assert_eq!(
        h.send("!queue show"),
        reply("Now: a\nNext: b\nthe queue is empty")
    );
    h.played(a);
    assert_eq!(h.send("!song current"), reply("b"));
    h.played(b);
    assert_eq!(h.send("!song current"), reply("c"));
}

#[tokio::test]
async fn clear_drops_the_end_of_a_decoded_track() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.finish_current();
    assert_eq!(h.send("!queue clear"), None);
    assert!(h.is_idle());
    assert_eq!(h.send("!song current"), reply("nothing is playing"));
}

#[tokio::test]
async fn stale_finished_is_ignored() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    let old = h.current_id();
    h.send("!song next");
    h.bot.finished(Ended {
        id: old,
        failure: None,
        end_frame: true,
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
    h.send("!song next");
    assert!(h.current_id().0 > first.0);
}

#[tokio::test]
async fn pause_holds_the_frames() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    assert_eq!(h.send("!song pause"), None);
    assert_eq!(h.paused(), Some("a"));
    assert!(h.control().paused);
    assert!(!h.current_stop().is_stopped());
}

#[tokio::test]
async fn pause_when_idle_does_nothing() {
    let mut h = Harness::new();
    assert_eq!(h.send("!song pause"), None);
    assert!(h.is_idle());
    assert!(!h.control().paused);
}

#[tokio::test]
async fn resume_continues_the_paused_track() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    let id = h.current_id();
    h.send("!song pause");
    assert_eq!(h.send("!song resume"), None);
    assert_eq!(h.playing(), Some("a"));
    assert_eq!(h.current_id(), id);
    assert!(!h.control().paused);
}

#[tokio::test]
async fn resume_when_idle_with_an_empty_queue_is_an_error() {
    let mut h = Harness::new();
    assert_eq!(
        h.send("!song resume"),
        Some("the queue is empty".to_string())
    );
    assert!(h.is_idle());
}

#[tokio::test]
async fn finished_while_paused_waits_idle_and_stays_paused() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.send("!song pause");
    h.finish_current();
    assert!(h.control().paused);
    assert_eq!(h.send("!song current"), reply("a (paused)"));
    assert_eq!(h.queued(), ["b"]);
}

#[tokio::test]
async fn resume_after_finishing_while_paused_starts_the_queue() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.send("!song pause");
    h.finish_current();
    assert_eq!(h.send("!song resume"), None);
    assert_eq!(h.playing(), Some("b"));
    assert!(!h.control().paused);
    assert_eq!(h.control().min_stream_id, 0);
}

#[tokio::test]
async fn add_after_finishing_while_paused_keeps_the_pause() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    h.send("!song pause");
    h.finish_current();
    h.add(&["b.mp3"]).await;
    assert!(h.control().paused);
    assert_eq!(h.playing(), None);
    assert_eq!(h.queued(), ["b"]);
    assert_eq!(h.send("!song current"), reply("a (paused)"));
    assert_eq!(h.send("!song resume"), None);
    assert!(!h.control().paused);
    assert_eq!(h.playing(), Some("b"));
    assert_eq!(h.send("!song current"), reply("a"));
}

#[tokio::test]
async fn pause_holds_the_end_of_the_last_track() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    let a = h.current_id();
    h.finish_current();
    assert_eq!(h.send("!song pause"), None);
    assert!(h.control().paused);
    assert_eq!(h.send("!song current"), reply("a (paused)"));
    assert!(!h.bot.player.is_playing());
    assert_eq!(h.send("!song resume"), None);
    assert!(!h.control().paused);
    assert_eq!(h.send("!song current"), reply("a"));
    h.played(a);
    assert!(h.is_idle());
}

#[tokio::test]
async fn an_end_heard_before_the_end_of_decoding_does_not_get_stuck() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    let a = h.current_id();
    h.played(a);
    h.finish_current_id(a);
    assert!(h.is_idle());
    assert_eq!(h.send("!song current"), reply("nothing is playing"));
}

#[tokio::test]
async fn an_end_heard_before_the_end_of_decoding_moves_on_to_the_next_song() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let a = h.current_id();
    h.played(a);
    h.finish_current_id(a);
    assert_eq!(h.playing(), Some("b"));
    assert_eq!(h.send("!song current"), reply("b"));
    assert_eq!(h.send("!queue show"), reply("Now: b\nthe queue is empty"));
}

#[tokio::test]
async fn next_cancels_the_track_and_drops_its_frames() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let old_id = h.current_id();
    let old_stop = h.current_stop();
    assert_eq!(h.send("!song next"), None);
    assert_eq!(h.playing(), Some("b"));
    assert!(old_stop.is_stopped());
    assert!(h.control().min_stream_id > old_id.0);
    assert!(h.control().min_stream_id <= h.current_id().0);
}

#[tokio::test]
async fn next_with_an_empty_queue_stops() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    let old_id = h.current_id();
    let old_stop = h.current_stop();
    assert_eq!(h.send("!song next"), None);
    assert!(h.is_idle());
    assert!(old_stop.is_stopped());
    assert!(h.control().min_stream_id > old_id.0);
}

#[tokio::test]
async fn next_while_paused_unpauses_for_the_next_track() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.send("!song pause");
    h.send("!song next");
    assert_eq!(h.playing(), Some("b"));
    assert!(!h.control().paused);
}

#[tokio::test]
async fn a_failed_track_is_heard_to_its_end_before_the_next() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let a = h.current_id();
    h.fail_current("playback failed");
    assert_eq!(h.playing(), Some("b"));
    assert_eq!(h.send("!song current"), reply("a"));
    assert_eq!(h.control().min_stream_id, 0);
    h.played(a);
    assert_eq!(h.send("!song current"), reply("b"));
}

#[tokio::test]
async fn next_during_the_rest_of_a_failed_track_skips_only_the_rest() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    h.fail_current("playback failed");
    assert_eq!(h.send("!song next"), None);
    assert_eq!(h.send("!song current"), reply("b"));
    assert_eq!(h.queued(), ["c"]);
}

#[tokio::test]
async fn a_track_whose_thread_panicked_is_dropped_and_the_next_starts() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let a = h.current_id();
    h.bot.finished(Ended {
        id: a,
        failure: Some("playback failed".to_owned()),
        end_frame: false,
    });
    assert_eq!(h.playing(), Some("b"));
    assert_eq!(h.send("!song current"), reply("b"));
    assert!(h.control().min_stream_id > a.0);
}

#[tokio::test]
async fn a_paused_track_whose_thread_panicked_leaves_the_queue_waiting() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let a = h.current_id();
    assert_eq!(h.send("!song pause"), None);
    h.bot.finished(Ended {
        id: a,
        failure: Some("playback failed".to_owned()),
        end_frame: false,
    });
    assert_eq!(h.send("!song current"), reply("nothing is playing"));
    assert_eq!(h.queued(), ["b"]);
    assert!(h.control().min_stream_id > a.0);
}

#[tokio::test]
async fn two_streams_heard_at_once_leave_nothing_waiting() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.finish_current();
    let b = h.current_id();
    h.played(b);
    assert_eq!(h.send("!song current"), reply("b"));
    h.finish_current_id(b);
    assert!(h.is_idle());
    assert_eq!(h.send("!song current"), reply("nothing is playing"));
}

#[tokio::test]
async fn a_tail_and_the_next_heard_at_once_move_on_to_the_third() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    h.finish_current();
    let b = h.current_id();
    h.played(b);
    h.finish_current_id(b);
    assert_eq!(h.playing(), Some("c"));
    assert_eq!(h.send("!song current"), reply("c"));
}
