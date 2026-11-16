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

/// The playback lives inside the states that have a track, so `Idle` cannot hold a stale one.
/// `Default` is there for `mem::take`, which moves the state out and leaves `Idle` behind.
// TODO: `Loading` for Tidal, `Departing` for presence.
#[derive(Debug, Default)]
pub(super) enum State {
    #[default]
    Idle,
    Playing(Playback),
    Paused(Playback),
}

impl State {
    pub(super) fn playback(&self) -> Option<&Playback> {
        match self {
            State::Idle => None,
            State::Playing(playback) | State::Paused(playback) => Some(playback),
        }
    }
}
