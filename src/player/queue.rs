//! The song queue and the queue operations of the player.

use std::{collections::VecDeque, num::NonZeroUsize};

use super::{Player, PlayerError};
use crate::{commands::QueuePosition, sources::TrackRef};

/// Songs waiting after the current one. Positions count from 1, as shown on the chat.
#[derive(Debug, Default)]
pub(super) struct Queue {
    tracks: VecDeque<TrackRef>,
    /// `None` means unlimited.
    max_len: Option<NonZeroUsize>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("the queue is full ({0} songs)")]
pub(crate) struct QueueFull(pub(crate) NonZeroUsize);

/// One page of `!queue show`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Page<'a> {
    pub(crate) number: NonZeroUsize,
    /// At least 1, an empty queue has one empty page.
    pub(crate) count: usize,
    /// Each track with its position, counted from 1.
    pub(crate) entries: Vec<(usize, &'a TrackRef)>,
}

impl Queue {
    pub(super) fn new(max_len: Option<NonZeroUsize>) -> Self {
        Self {
            tracks: VecDeque::new(),
            max_len,
        }
    }

    pub(super) fn push(&mut self, track: TrackRef) -> Result<(), QueueFull> {
        if let Some(max) = self.max_len
            && self.tracks.len() >= max.get()
        {
            return Err(QueueFull(max));
        }
        self.tracks.push_back(track);
        Ok(())
    }

    pub(super) fn pop_next(&mut self) -> Option<TrackRef> {
        self.tracks.pop_front()
    }

    /// `position` as shown by `!queue show`. `None` when there is no such position.
    pub(super) fn remove(&mut self, position: NonZeroUsize) -> Option<TrackRef> {
        self.tracks.remove(position.get() - 1)
    }

    pub(super) fn remove_last(&mut self) -> Option<TrackRef> {
        self.tracks.pop_back()
    }

    pub(super) fn clear(&mut self) {
        self.tracks.clear();
    }

    pub(super) fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// `None` when the page is past the end.
    pub(super) fn page(&self, number: NonZeroUsize, size: NonZeroUsize) -> Option<Page<'_>> {
        let count = self.tracks.len().div_ceil(size.get()).max(1);
        if number.get() > count {
            return None;
        }
        let start = (number.get() - 1) * size.get();
        let entries = self
            .tracks
            .iter()
            .enumerate()
            .skip(start)
            .take(size.get())
            .map(|(index, track)| (index + 1, track))
            .collect();
        Some(Page {
            number,
            count,
            entries,
        })
    }
}

impl Player {
    /// A page of the queue, `None` when the page is past the end.
    pub fn queue_page(&self, number: NonZeroUsize, size: NonZeroUsize) -> Option<Page<'_>> {
        self.queue.page(number, size)
    }

    /// `position` as shown by `!queue show`, or the last song.
    pub fn queue_remove(&mut self, position: QueuePosition) -> Result<(), PlayerError> {
        match position {
            QueuePosition::Number(number) => match self.queue.remove(number) {
                Some(_) => Ok(()),
                None => Err(PlayerError::NoSuchPosition(number)),
            },
            QueuePosition::Last => match self.queue.remove_last() {
                Some(_) => Ok(()),
                None => Err(PlayerError::QueueEmpty),
            },
        }
    }

    /// Empties the queue and stops the current track.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.stop_current();
    }
}
