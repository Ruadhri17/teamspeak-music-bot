//! Player state and playback IDs.

use tokio_util::sync::CancellationToken;

use crate::sources::TrackRef;

/// Identifies one playback, so a late "finished" from an old track can be told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlaybackId(pub(crate) u64);

/// Sent from the audio thread when a track ends.
#[derive(Debug)]
pub(crate) struct Ended {
    pub(crate) id: PlaybackId,
    /// `Some` when the track failed, with a reason that is safe to show on the chat.
    pub(crate) failure: Option<String>,
}

/// The current track with everything needed to stop it.
#[derive(Debug)]
pub(super) struct Playback {
    pub(super) id: PlaybackId,
    pub(super) stop: CancellationToken,
    pub(super) track: TrackRef,
}

/// `Default` (`Idle`) is required by `mem::take`.
// TODO: `Departing` for presence.
#[derive(Debug, Default)]
pub(super) enum State {
    #[default]
    Idle,
    Playing(Playback),
    Paused(Playback),
}

impl State {
    /// The current track, `None` when idle.
    pub(super) fn playback(&self) -> Option<&Playback> {
        match self {
            State::Idle => None,
            State::Playing(playback) | State::Paused(playback) => Some(playback),
        }
    }
}
