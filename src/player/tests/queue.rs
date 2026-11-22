use std::num::NonZeroUsize;

use crate::{
    player::queue::{Queue, QueueFull},
    sources::{Location, TrackRef},
};

fn track(title: &str) -> TrackRef {
    TrackRef {
        location: Location::Local(format!("{title}.mp3").into()),
        title: title.to_string(),
    }
}

fn n(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

fn queue_of(titles: &[&str]) -> Queue {
    let mut queue = Queue::new(None);
    for title in titles {
        queue.push(track(title)).unwrap();
    }
    queue
}

fn titles(queue: &Queue, page: usize, size: usize) -> Vec<(usize, String)> {
    queue
        .page(n(page), n(size))
        .unwrap()
        .entries
        .into_iter()
        .map(|(position, track)| (position, track.title.clone()))
        .collect()
}

#[tokio::test]
async fn pop_next_keeps_the_order() {
    let mut queue = queue_of(&["a", "b", "c"]);
    assert_eq!(queue.pop_next(), Some(track("a")));
    assert_eq!(queue.pop_next(), Some(track("b")));
    assert_eq!(queue.pop_next(), Some(track("c")));
    assert_eq!(queue.pop_next(), None);
}

#[tokio::test]
async fn remove_counts_from_one() {
    let mut queue = queue_of(&["a", "b", "c"]);
    assert_eq!(queue.remove(n(2)), Some(track("b")));
    assert_eq!(queue.pop_next(), Some(track("a")));
    assert_eq!(queue.pop_next(), Some(track("c")));
}

#[tokio::test]
async fn remove_past_the_end_is_none() {
    let mut queue = queue_of(&["a"]);
    assert_eq!(queue.remove(n(2)), None);
    assert_eq!(queue.pop_next(), Some(track("a")));
}

#[tokio::test]
async fn clear_empties_the_queue() {
    let mut queue = queue_of(&["a", "b"]);
    queue.clear();
    assert!(queue.is_empty());
    assert_eq!(queue.pop_next(), None);
}

#[tokio::test]
async fn max_len_rejects_extra_tracks() {
    let mut queue = Queue::new(Some(n(2)));
    queue.push(track("a")).unwrap();
    queue.push(track("b")).unwrap();
    assert_eq!(queue.push(track("c")), Err(QueueFull(n(2))));
    queue.pop_next();
    assert_eq!(queue.push(track("c")), Ok(()));
}

#[tokio::test]
async fn queue_full_message() {
    assert_eq!(
        QueueFull(n(500)).to_string(),
        "the queue is full (500 songs)"
    );
}

#[tokio::test]
async fn empty_queue_has_one_empty_page() {
    let queue = Queue::new(None);
    let page = queue.page(n(1), n(15)).unwrap();
    assert_eq!(page.count, 1);
    assert!(page.entries.is_empty());
    assert_eq!(queue.page(n(2), n(15)), None);
}

#[tokio::test]
async fn pages_are_numbered_with_positions() {
    let queue = queue_of(&["a", "b", "c", "d", "e"]);
    assert_eq!(queue.page(n(1), n(2)).unwrap().count, 3);
    assert_eq!(titles(&queue, 1, 2), [(1, "a".into()), (2, "b".into())]);
    assert_eq!(titles(&queue, 2, 2), [(3, "c".into()), (4, "d".into())]);
    assert_eq!(titles(&queue, 3, 2), [(5, "e".into())]);
    assert_eq!(queue.page(n(4), n(2)), None);
}

#[tokio::test]
async fn full_last_page_adds_no_empty_page() {
    let queue = queue_of(&["a", "b", "c", "d"]);
    assert_eq!(queue.page(n(1), n(2)).unwrap().count, 2);
    assert_eq!(queue.page(n(3), n(2)), None);
}
