//! Player: plays tracks and keeps the queue. Knows nothing about the chat or TeamSpeak,
//! the bot calls its methods and turns the results into replies.
//!
//! Every playback has a `PlaybackId` and a `CancellationToken`. Results with a stale ID are dropped.
//!
//! - `mod.rs`: `Player`, its errors and what it reports
//! - `launcher`: `Launcher`, the interface to the audio thread
//! - `state`: `State` and `PlaybackId`
//! - `song`: the playback transitions (start, pause, next, finish) and the volume
//! - `queue`: the song queue as a plain data structure and the queue operations

mod launcher;
mod queue;
mod song;
mod state;

pub use launcher::{AudioLauncher, Launcher};
pub(crate) use queue::QueueFull;
pub(crate) use state::Ended;
#[cfg(test)]
pub(crate) use {song::chat_reason, state::PlaybackId};

use std::{num::NonZeroUsize, sync::Arc};

use tokio::sync::{mpsc, watch};

use crate::{
    config::{Config, SourcesConfig, Volume},
    sources::TrackRef,
    teamspeak::AudioControl,
};
use queue::Queue;
use state::State;

/// A refused request, the text is meant for the chat.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlayerError {
    #[error("the queue is empty")]
    QueueEmpty,
    #[error("no song at position {0} in the queue")]
    NoSuchPosition(NonZeroUsize),
    #[error("the highest volume is {0}")]
    VolumeTooHigh(u8),
}

/// Something that happened during playback that the bot should know about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A track failed to open or to play. `reason` is safe to show on the chat.
    CouldNotPlay { title: String, reason: String },
}

/// The result of `Player::enqueue`.
#[derive(Debug)]
pub struct Enqueued {
    pub added: usize,
    /// Set when the queue filled up before all tracks were added.
    pub full: Option<QueueFull>,
}

/// The track that plays now, for `!song current` and `!queue show`.
#[derive(Debug, Clone, Copy)]
pub struct Current<'a> {
    pub track: &'a TrackRef,
    pub paused: bool,
}

pub struct Player {
    state: State,
    /// Songs after the current one.
    queue: Queue,
    /// The ID for the next playback, only ever goes up.
    next_id: u64,
    /// Starts the audio of a track, `AudioLauncher` outside of tests.
    launcher: Box<dyn Launcher>,
    /// Tells the TeamSpeak client to hold the frames (pause) or drop a stopped track's frames.
    control: watch::Sender<AudioControl>,
    /// Audio threads report here when the track ends.
    done_tx: mpsc::Sender<Ended>,
    done_rx: mpsc::Receiver<Ended>,
    /// What happened since the bot last took them, in order.
    notices: Vec<Notice>,
    /// For opening a track right before it plays. Shared with the bot's lookups.
    sources: Arc<SourcesConfig>,
    /// `set_volume` above this is refused.
    max_volume: Volume,
}

impl Player {
    pub fn new(
        control: watch::Sender<AudioControl>,
        launcher: Box<dyn Launcher>,
        sources: Arc<SourcesConfig>,
        config: &Config,
    ) -> Self {
        let (done_tx, done_rx) = mpsc::channel(8);
        // The player owns the volume, the client only reads it from `control`.
        control.send_modify(|control| control.volume = config.audio.default_volume);
        Self {
            state: State::Idle,
            queue: Queue::new(config.queue.max_len),
            next_id: 0,
            launcher,
            control,
            done_tx,
            done_rx,
            notices: Vec::new(),
            sources,
            max_volume: config.audio.max_volume,
        }
    }

    /// The next track that ended, for the bot's `select!`. Pass it to `finished`.
    /// Never ends, the player keeps a sender itself.
    pub async fn ended(&mut self) -> Ended {
        self.done_rx
            .recv()
            .await
            .expect("the player keeps a sender, the channel cannot close")
    }

    /// What happened since the last call, in order.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    /// The current track, `None` while idle.
    pub fn current(&self) -> Option<Current<'_>> {
        match &self.state {
            State::Idle => None,
            State::Playing(playback) => Some(Current {
                track: &playback.track,
                paused: false,
            }),
            State::Paused(playback) => Some(Current {
                track: &playback.track,
                paused: true,
            }),
        }
    }
}

/// For the bot's tests, which check the state through the player.
#[cfg(test)]
impl Player {
    pub(crate) fn playing(&self) -> Option<&str> {
        match &self.state {
            State::Playing(playback) => Some(&playback.track.title),
            _ => None,
        }
    }

    pub(crate) fn paused(&self) -> Option<&str> {
        match &self.state {
            State::Paused(playback) => Some(&playback.track.title),
            _ => None,
        }
    }

    /// Nothing plays and nothing is paused.
    pub(crate) fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle)
    }

    pub(crate) fn current_id(&self) -> PlaybackId {
        self.state.playback().expect("a track plays").id
    }

    pub(crate) fn current_stop(&self) -> tokio_util::sync::CancellationToken {
        self.state.playback().expect("a track plays").stop.clone()
    }

    pub(crate) fn queued(&self) -> Vec<String> {
        let all = NonZeroUsize::new(usize::MAX).expect("not zero");
        self.queue
            .page(NonZeroUsize::MIN, all)
            .expect("the first page always exists")
            .entries
            .into_iter()
            .map(|(_, track)| track.title.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests;
