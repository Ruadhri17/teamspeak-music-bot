//! Player state and playback IDs.

use tokio_util::sync::CancellationToken;

/// Identifies one playback, so a late "finished" from an old track can be told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlaybackId(pub(super) u64);

/// The token and ID live inside the states that have a track, so `Idle` cannot hold a stale one.
// TODO: `Loading` for Tidal, `Departing` for presence.
#[derive(Debug)]
pub(super) enum State {
    Idle,
    Playing {
        id: PlaybackId,
        stop: CancellationToken,
    },
    Paused {
        id: PlaybackId,
        stop: CancellationToken,
    },
}
