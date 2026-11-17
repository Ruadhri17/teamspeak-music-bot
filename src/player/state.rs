//! Player state and playback IDs.

use tokio_util::sync::CancellationToken;

use crate::sources::TrackRef;

/// Identifies one playback, so a late "finished" from an old track can be told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlaybackId(pub(super) u64);

/// The current track with everything needed to stop it.
#[derive(Debug)]
pub(super) struct Playback {
    pub(super) id: PlaybackId,
    pub(super) stop: CancellationToken,
    pub(super) track: TrackRef,
}

/// `Default` (`Idle`) is required by `mem::take`.
// TODO: `Loading` for Tidal, `Departing` for presence.
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
