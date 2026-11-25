use std::num::NonZeroUsize;

use super::harness::{Harness, on_channel};
use crate::teamspeak::ReplyTo;

const SCRIPT: &str = r#"
for arg; do target=$arg; done
entry() { printf '{"_type": "url", "id": "%s", "title": "%s"}' "$1" "$2"; }
three="$(entry aaaaaaaaaa1 one),$(entry aaaaaaaaaa2 two),$(entry aaaaaaaaaa3 three)"
case $target in
    *list=PLthree) printf '{"_type": "playlist", "title": "Three", "playlist_count": 3, "entries": [%s]}' "$three" ;;
    *list=PLbig) printf '{"_type": "playlist", "title": "Big", "playlist_count": 10, "entries": [%s]}' "$three" ;;
    *list=PLone) printf '{"_type": "playlist", "title": "One", "playlist_count": 1, "entries": [%s]}' "$(entry aaaaaaaaaa1 one)" ;;
    *list=PLbbcode) printf '{"_type": "playlist", "title": "[b]x", "entries": [%s]}' "$(entry aaaaaaaaaa1 one)" ;;
    *) printf '{"_type": "video", "id": "bbbbbbbbbbb", "title": "%s"}' "${target#ytsearch1:}" ;;
esac
"#;

const LINK: &str = "https://www.youtube.com/playlist?list=";

fn harness() -> Harness {
    Harness::with_yt_dlp(SCRIPT)
}

fn with_max_len(max_len: usize) -> Harness {
    Harness::with_yt_dlp_and(SCRIPT, |config| {
        config.queue.max_len = NonZeroUsize::new(max_len);
    })
}

#[tokio::test]
async fn playlist_adds_every_song_in_order_and_says_so() {
    let mut h = harness();
    let outbox = h.add_outbox(&format!("!song add {LINK}PLthree")).await;
    assert_eq!(outbox, on_channel("added 3 songs from Three"));
    assert_eq!(h.playing(), Some("one"));
    assert_eq!(h.queued(), ["two", "three"]);
}

#[tokio::test]
async fn one_song_playlist_says_song() {
    let mut h = harness();
    let outbox = h.add_outbox(&format!("!song add {LINK}PLone")).await;
    assert_eq!(outbox, on_channel("added 1 song from One"));
}

#[tokio::test]
async fn playlist_over_the_limit_says_how_much_was_added() {
    let mut h = harness();
    let outbox = h.add_outbox(&format!("!song add {LINK}PLbig")).await;
    assert_eq!(outbox, on_channel("added 3 of 10 songs from Big"));
    assert_eq!(h.queued(), ["two", "three"]);
}

#[tokio::test]
async fn full_queue_in_the_middle_of_a_playlist() {
    let mut h = with_max_len(1);
    let outbox = h.add_outbox(&format!("!song add {LINK}PLthree")).await;
    assert_eq!(
        outbox,
        on_channel("the queue is full (1 songs), added 2 of 3 songs from Three")
    );
    assert_eq!(h.playing(), Some("one"));
    assert_eq!(h.queued(), ["two"]);
}

#[tokio::test]
async fn full_queue_before_a_playlist() {
    let mut h = with_max_len(1);
    assert!(h.add_outbox("!song add first").await.is_empty());
    assert!(h.add_outbox("!song add second").await.is_empty());
    let outbox = h.add_outbox(&format!("!song add {LINK}PLthree")).await;
    assert_eq!(outbox, on_channel("the queue is full (1 songs)"));
    assert_eq!(h.playing(), Some("first"));
    assert_eq!(h.queued(), ["second"]);
}

#[tokio::test]
async fn single_video_is_silent() {
    let mut h = harness();
    assert!(h.add_outbox("!song add get lucky").await.is_empty());
    assert_eq!(h.playing(), Some("get lucky"));
}

#[tokio::test]
async fn playlist_title_cannot_format_the_chat() {
    let mut h = harness();
    let outbox = h.add_outbox(&format!("!song add {LINK}PLbbcode")).await;
    assert_eq!(outbox, on_channel("added 1 song from \\[b\\]x"));
}

#[tokio::test]
async fn playlist_summary_goes_to_the_sender() {
    let mut h = harness();
    let sender = ReplyTo::Client(7);
    let text = format!("!song add {LINK}PLone");
    assert_eq!(h.send_from(sender, &text), None);
    h.settle().await;
    assert_eq!(h.outbox(), [(sender, "added 1 song from One".to_owned())]);
}
