//! Player: the actor that owns the playback state and the queue.
//!
//! States: Idle / Loading / Playing / Paused / Departing.
//! Every playback has a `PlaybackId` and a `CancellationToken`. Results with a stale ID are dropped.
//! Handles `!song ...` and `!queue ...` commands.

mod queue;
