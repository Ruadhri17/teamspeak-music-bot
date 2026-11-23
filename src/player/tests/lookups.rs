use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

use super::harness::Harness;
use crate::client::ReplyTo;

const SCRIPT: &str = r#"
for arg; do target=$arg; done
phrase=${target#ytsearch1:}
dir=$(dirname "$0")
case $phrase in
    slow*) sleep 1 ;;
    hang*) echo $$ > "$dir/hang.pid"; exec sleep 30 ;;
    fail*) echo "ERROR: [youtube] something went wrong" >&2; exit 1 ;;
esac
printf '{"_type": "video", "id": "aaaaaaaaaaa", "title": "%s"}' "$phrase"
"#;

fn harness() -> Harness {
    Harness::with_yt_dlp(SCRIPT)
}

async fn wait_for(path: &Path) -> String {
    let start = Instant::now();
    loop {
        if let Ok(text) = fs::read_to_string(path)
            && !text.is_empty()
        {
            return text.trim().to_owned();
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{path:?} never appeared"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn is_running(pid: &str) -> bool {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat
            .rsplit(") ")
            .next()
            .is_some_and(|rest| !rest.starts_with('Z')),
        Err(_) => false,
    }
}

#[tokio::test]
async fn commands_answer_while_a_lookup_runs() {
    let mut h = harness();
    let start = Instant::now();
    assert_eq!(h.send("!song add slow"), None);
    assert!(h.send("!queue show").is_some());
    assert!(start.elapsed() < Duration::from_millis(500));
    assert!(h.is_idle());

    h.settle().await;
    assert_eq!(h.playing(), Some("slow"));
}

#[tokio::test]
async fn track_end_during_a_lookup_starts_the_next_track() {
    let mut h = harness();
    for name in ["a.mp3", "b.mp3"] {
        assert_eq!(h.send(&format!("!song add --source local {name}")), None);
    }
    h.settle().await;
    assert_eq!(h.send("!song add slow"), None);

    h.finish_current();
    assert_eq!(h.playing(), Some("b"));
    assert!(h.queued().is_empty());

    h.settle().await;
    assert_eq!(h.queued(), ["slow"]);
}

#[tokio::test]
async fn tracks_keep_the_order_of_the_commands() {
    let mut h = harness();
    assert_eq!(h.send("!song add slow one"), None);
    assert_eq!(h.send("!song add fast"), None);

    h.next_lookup().await;
    assert!(h.is_idle());
    assert!(h.queued().is_empty());

    h.next_lookup().await;
    assert_eq!(h.playing(), Some("slow one"));
    assert_eq!(h.queued(), ["fast"]);
}

#[tokio::test]
async fn failed_lookup_does_not_hold_back_the_next_one() {
    let mut h = harness();
    assert_eq!(h.send("!song add fail"), None);
    assert_eq!(h.send("!song add fast"), None);
    h.settle().await;

    assert_eq!(h.playing(), Some("fast"));
    let outbox = h.outbox();
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0].0, ReplyTo::Channel);
}

#[tokio::test]
async fn lookup_error_goes_to_the_sender() {
    let mut h = harness();
    let sender = ReplyTo::Client(7);
    assert_eq!(h.send_from(sender, "!song add fail"), None);
    h.settle().await;

    let outbox = h.outbox();
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0].0, sender);
    assert!(h.is_idle());
}

#[tokio::test]
async fn queue_clear_cancels_lookups_and_kills_yt_dlp() {
    let mut h = harness();
    assert_eq!(h.send("!song add hang"), None);
    let pid_file = h.scripts.as_ref().unwrap().path().join("hang.pid");
    let pid = wait_for(&pid_file).await;
    assert!(is_running(&pid));

    assert_eq!(h.send("!queue clear"), None);
    tokio::time::timeout(Duration::from_secs(2), h.settle())
        .await
        .expect("the lookup was not cancelled");

    let start = Instant::now();
    while is_running(&pid) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "yt-dlp still runs"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(h.is_idle());
    assert!(h.queued().is_empty());
    assert!(h.outbox().is_empty());
}
