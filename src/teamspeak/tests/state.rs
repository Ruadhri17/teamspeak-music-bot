use crate::teamspeak::{CurrentChannel, state::state};

fn channel(id: u64, others: usize) -> Option<CurrentChannel> {
    Some(CurrentChannel {
        id,
        at_home: id == 1,
        others,
    })
}

#[test]
fn only_a_different_channel_is_a_change() {
    let (tx, mut rx) = state();
    tx.channel(channel(2, 0));
    assert!(rx.channel.has_changed().unwrap());
    assert_eq!(*rx.channel.borrow_and_update(), channel(2, 0));
    tx.channel(channel(2, 0));
    assert!(!rx.channel.has_changed().unwrap());
    tx.channel(channel(2, 1));
    assert_eq!(*rx.channel.borrow_and_update(), channel(2, 1));
}

#[test]
fn the_bot_sees_the_latest_channel_however_many_came_before() {
    let (tx, mut rx) = state();
    for others in 0..100 {
        tx.channel(channel(2, others));
    }
    assert_eq!(*rx.channel.borrow_and_update(), channel(2, 99));
}

#[test]
fn the_same_channel_after_a_reconnect_is_a_change() {
    let (tx, mut rx) = state();
    tx.channel(channel(1, 0));
    rx.channel.borrow_and_update();
    tx.channel(None);
    tx.channel(channel(1, 0));
    assert!(rx.channel.has_changed().unwrap());
    assert_eq!(*rx.channel.borrow_and_update(), channel(1, 0));
}

#[test]
fn every_connect_counts() {
    let (tx, mut rx) = state();
    tx.connected();
    assert!(rx.connects.has_changed().unwrap());
    assert_eq!(*rx.connects.borrow_and_update(), 1);
    tx.connected();
    assert_eq!(*rx.connects.borrow_and_update(), 2);
}

#[test]
fn heard_only_goes_up() {
    let (tx, mut rx) = state();
    tx.heard(3);
    tx.heard(5);
    assert_eq!(*rx.heard.borrow_and_update(), Some(5));
    tx.heard(4);
    assert!(!rx.heard.has_changed().unwrap());
    assert_eq!(*rx.heard.borrow(), Some(5));
}

#[test]
fn the_first_stream_counts_as_heard() {
    let (tx, mut rx) = state();
    tx.heard(0);
    assert_eq!(*rx.heard.borrow_and_update(), Some(0));
}
