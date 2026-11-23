//! Player state and playback IDs.

use tokio::task::Id;
use tokio_util::sync::CancellationToken;

use crate::{client::ReplyTo, sources::TrackRef};

/// Identifies one playback, so a late "finished" from an old track can be told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlaybackId(pub(super) u64);

/// Sent from the audio thread when a track ends.
#[derive(Debug)]
pub(super) struct Ended {
    pub(super) id: PlaybackId,
    /// `Some` when the track failed, with a reason that is safe to show on the chat.
    pub(super) failure: Option<String>,
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

/// A `!song add` whose lookup runs in `Player::lookups`, kept in the order of the commands.
#[derive(Debug)]
pub(super) struct Pending {
    /// The lookup task, matched with the ID that `join_next_with_id` returns.
    pub(super) task: Id,
    /// Where an error goes, the place the command came from.
    pub(super) reply_to: ReplyTo,
    /// `None` while the lookup runs.
    pub(super) result: Option<Result<TrackRef, String>>,
}
