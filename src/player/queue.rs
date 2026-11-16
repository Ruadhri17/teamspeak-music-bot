//! The song queue and the `!queue ...` commands.
//!
//! `Queue` holds the songs waiting after the current one. It knows nothing about
//! the player or async, so it is tested on its own. The `impl Player` block below
//! turns `!queue show|remove|clear` into calls on it.

use std::{collections::VecDeque, num::NonZeroUsize};

use super::Player;
use crate::sources::TrackRef;

/// Songs waiting after the current one. Positions as the user sees them count from 1,
/// the conversion to `VecDeque` indexes happens only here.
#[derive(Debug, Default)]
pub(super) struct Queue {
    tracks: VecDeque<TrackRef>,
    /// `None` means unlimited.
    max_len: Option<NonZeroUsize>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("the queue is full ({0} songs)")]
pub(super) struct QueueFull(pub(super) NonZeroUsize);

/// One page of `!queue show`.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Page<'a> {
    pub(super) number: NonZeroUsize,
    /// At least 1, an empty queue has one empty page.
    pub(super) count: usize,
    /// Each track with its position, counted from 1.
    pub(super) entries: Vec<(usize, &'a TrackRef)>,
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
    /// `!queue show [page]`. Always answers, it is an informational command.
    // TODO: TS3 messages are limited to about 1024 characters, long titles need cutting.
    pub(super) fn queue_show(&self, page: NonZeroUsize) -> String {
        let Some(page) = self.queue.page(page, self.page_size) else {
            return "no such page, the queue is shorter".to_string();
        };

        let mut lines = Vec::new();
        if let Some(playback) = self.state.playback() {
            lines.push(format!("Now: {}", playback.track.title));
        }
        if page.entries.is_empty() {
            lines.push("the queue is empty".to_string());
        }
        for (position, track) in &page.entries {
            lines.push(format!("{position}. {}", track.title));
        }
        if page.count > 1 {
            lines.push(format!("page {}/{}", page.number, page.count));
        }
        lines.join("\n")
    }

    /// `!queue remove <number>`. `Err` is the message for the chat.
    pub(super) fn queue_remove(&mut self, position: NonZeroUsize) -> Result<(), String> {
        match self.queue.remove(position) {
            Some(_) => Ok(()),
            None => Err(format!("no song at position {position} in the queue")),
        }
    }

    /// `!queue clear`. Empties the queue and stops the current track too.
    pub(super) fn queue_clear(&mut self) {
        self.queue.clear();
        self.stop_current();
    }
}
