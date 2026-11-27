use tokio::sync::mpsc;

use crate::teamspeak::{CurrentChannel, TeamspeakEvent, reports::Reports};

fn channel(id: u64, others: usize) -> CurrentChannel {
    CurrentChannel {
        id,
        at_home: id == 1,
        others,
    }
}

fn received(rx: &mut mpsc::Receiver<TeamspeakEvent>) -> Vec<String> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .map(|event| match event {
            TeamspeakEvent::Channel(channel) => {
                format!("channel {} {}", channel.id, channel.others)
            }
            TeamspeakEvent::Connected => "connected".to_owned(),
            other => format!("{other:?}"),
        })
        .collect()
}

fn full() -> (mpsc::Sender<TeamspeakEvent>, mpsc::Receiver<TeamspeakEvent>) {
    let (tx, rx) = mpsc::channel(1);
    tx.try_send(TeamspeakEvent::Played(0)).unwrap();
    (tx, rx)
}

#[test]
fn a_change_is_sent_once() {
    let (tx, mut rx) = mpsc::channel(8);
    let mut reports = Reports::default();
    reports.channel(&tx, channel(2, 0));
    reports.channel(&tx, channel(2, 0));
    reports.channel(&tx, channel(2, 1));
    assert_eq!(received(&mut rx), ["channel 2 0", "channel 2 1"]);
}

#[test]
fn a_channel_the_bot_had_no_room_for_goes_out_on_a_later_tick() {
    let (tx, mut rx) = full();
    let mut reports = Reports::default();
    reports.channel(&tx, channel(2, 3));
    reports.retry(&tx);
    assert_eq!(received(&mut rx), ["Played(0)"]);
    reports.retry(&tx);
    assert_eq!(received(&mut rx), ["channel 2 3"]);
    reports.retry(&tx);
    assert!(received(&mut rx).is_empty());
}

#[test]
fn only_the_latest_waiting_channel_is_sent() {
    let (tx, mut rx) = full();
    let mut reports = Reports::default();
    reports.channel(&tx, channel(2, 0));
    reports.channel(&tx, channel(3, 2));
    received(&mut rx);
    reports.retry(&tx);
    assert_eq!(received(&mut rx), ["channel 3 2"]);
}

#[test]
fn a_waiting_change_back_to_what_the_bot_has_is_dropped() {
    let (tx, mut rx) = mpsc::channel(1);
    let mut reports = Reports::default();
    reports.channel(&tx, channel(2, 0));
    reports.channel(&tx, channel(2, 4));
    reports.channel(&tx, channel(2, 0));
    received(&mut rx);
    reports.retry(&tx);
    assert!(received(&mut rx).is_empty());
}

#[test]
fn connected_the_bot_had_no_room_for_goes_out_on_a_later_tick() {
    let (tx, mut rx) = full();
    let mut reports = Reports::default();
    reports.channel(&tx, channel(1, 0));
    reports.connected(&tx);
    received(&mut rx);
    reports.retry(&tx);
    assert_eq!(received(&mut rx), ["channel 1 0"]);
    reports.retry(&tx);
    assert_eq!(received(&mut rx), ["connected"]);
    reports.retry(&tx);
    assert!(received(&mut rx).is_empty());
}

#[test]
fn a_reconnect_sends_the_same_channel_again_and_drops_what_waited() {
    let (tx, mut rx) = mpsc::channel(8);
    let mut reports = Reports::default();
    reports.channel(&tx, channel(1, 0));
    received(&mut rx);

    let (full_tx, mut full_rx) = full();
    reports.connected(&full_tx);
    reports.reset();
    received(&mut full_rx);
    reports.retry(&full_tx);
    assert!(received(&mut full_rx).is_empty());

    reports.channel(&tx, channel(1, 0));
    assert_eq!(received(&mut rx), ["channel 1 0"]);
}

#[test]
fn a_closed_bot_is_not_retried() {
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let mut reports = Reports::default();
    reports.channel(&tx, channel(2, 0));
    reports.connected(&tx);
    reports.retry(&tx);
}
