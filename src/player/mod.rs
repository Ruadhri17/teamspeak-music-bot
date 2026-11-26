//! Player: plays tracks and keeps the queue. Knows nothing about the chat or TeamSpeak,
//! the bot calls its methods and turns the results into replies.
//!
//! Every playback has a `PlaybackId` and a `Stopper`. Results with a stale ID are dropped.
//!
//! The audio thread decodes ahead of what is heard. A track that finished decoding moves to
//! `tails` while its end still waits in the buffer, and the next one starts decoding. What
//! is heard is the oldest tail, until the client reports that its last frame went out.
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

use std::{collections::VecDeque, num::NonZeroUsize, sync::Arc};

use tokio::sync::{mpsc, watch};

use crate::{
    config::{Config, SourcesConfig, Volume},
    sources::TrackRef,
    teamspeak::AudioControl,
};
use queue::Queue;
use state::{Kind, Playback, State};

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
    /// A song from the queue started to be heard, `current` shows it. Not sent for the
    /// sound of an event.
    SongStarted,
    /// The sound of an event played to its end, or failed while playing.
    SoundEnded,
    /// The sound of an event was stopped before its end: `next`, `clear`, `stop_sound`
    /// or another sound.
    SoundInterrupted,
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
    /// The playback that decodes now.
    state: State,
    /// Tracks that finished decoding while their end is still in the buffer, oldest first.
    /// The first one is heard now, before the track of `state`.
    tails: VecDeque<Playback>,
    /// The client sent the last frame of the current track before the player heard that
    /// it finished decoding: the two come through different channels. Its `finished`
    /// then moves on at once instead of waiting for an end that already came.
    played_early: Option<state::PlaybackId>,
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
            tails: VecDeque::new(),
            played_early: None,
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

    /// Music plays, also the end of a decoded track. A paused track and the sound of an
    /// event do not count.
    pub fn is_playing(&self) -> bool {
        matches!(&self.state, State::Playing(playback) if playback.kind == Kind::Track)
            || (!self.tails.is_empty() && !self.control.borrow().paused)
    }

    pub fn queue_is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Nothing plays and nothing is paused.
    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle) && self.tails.is_empty()
    }

    /// The track heard now. `None` while idle or while the sound of an event plays.
    pub fn current(&self) -> Option<Current<'_>> {
        if let Some(tail) = self.tails.front() {
            return Some(Current {
                track: &tail.track,
                paused: self.control.borrow().paused,
            });
        }
        match &self.state {
            State::Playing(playback) if playback.kind == Kind::Track => Some(Current {
                track: &playback.track,
                paused: false,
            }),
            State::Paused(playback) => Some(Current {
                track: &playback.track,
                paused: true,
            }),
            _ => None,
        }
    }
}

impl Player {
    /// The track after the one heard now, while that is the end of a decoded track. It
    /// decodes ahead already, so it is no longer in the queue.
    pub fn up_next(&self) -> Option<&TrackRef> {
        if self.tails.is_empty() {
            return None;
        }
        if let Some(tail) = self.tails.get(1) {
            return Some(&tail.track);
        }
        match &self.state {
            State::Playing(playback) | State::Paused(playback) if playback.kind == Kind::Track => {
                Some(&playback.track)
            }
            _ => None,
        }
    }

    /// Only the end of a decoded track is left and the client holds it: `pause` while
    /// nothing decodes any more.
    fn tail_paused(&self) -> bool {
        matches!(self.state, State::Idle) && !self.tails.is_empty() && self.control.borrow().paused
    }

    /// A track is heard now: the oldest tail, or the playing track when no tail is left.
    fn track_heard(&self) -> bool {
        !self.tails.is_empty()
            || matches!(&self.state, State::Playing(playback) if playback.kind == Kind::Track)
    }
}

/// For the bot's tests, which check the state through the player.
#[cfg(test)]
impl Player {
    pub(crate) fn playing(&self) -> Option<&str> {
        match &self.state {
            State::Playing(playback) if playback.kind == Kind::Track => Some(&playback.track.title),
            _ => None,
        }
    }

    /// The title of the event sound that plays now.
    pub(crate) fn sound(&self) -> Option<&str> {
        match &self.state {
            State::Playing(playback) if playback.kind == Kind::Sound => Some(&playback.track.title),
            _ => None,
        }
    }

    pub(crate) fn paused(&self) -> Option<&str> {
        match &self.state {
            State::Paused(playback) => Some(&playback.track.title),
            _ => None,
        }
    }

    pub(crate) fn current_id(&self) -> PlaybackId {
        self.state.playback().expect("a track plays").id
    }

    pub(crate) fn current_stop(&self) -> crate::process::Stopper {
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
