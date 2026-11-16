//! `!song ...` commands and the playback transitions shared with `queue`:
//! starting, stopping and finishing tracks.

use std::mem;

use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::{
    Player,
    state::{Playback, PlaybackId, State},
};
use crate::{
    audio::AudioInput,
    config::SourceKind,
    sources::{Location, TrackRef, local},
};

impl Player {
    /// `!song add`. Appends to the queue and starts it when nothing plays.
    /// A paused track stays paused. `Err` is the message for the chat.
    pub(super) fn add(&mut self, query: &str, source: Option<SourceKind>) -> Result<(), String> {
        let track = self.resolve(query, source)?;
        self.queue.push(track).map_err(|err| err.to_string())?;
        if let State::Idle = self.state {
            self.play_next();
        }
        Ok(())
    }

    /// `!song pause`. Pauses a playing track, otherwise does nothing.
    pub(super) fn pause(&mut self) {
        self.state = match mem::take(&mut self.state) {
            State::Playing(playback) => {
                self.control.send_modify(|control| control.paused = true);
                State::Paused(playback)
            }
            other => other,
        };
    }

    /// `!song resume`. Resumes a paused track, or starts the queue when idle.
    /// Unpauses the client even when idle, a track that finished while paused
    /// may still have frames in the buffer.
    pub(super) fn resume(&mut self) -> Result<(), String> {
        self.control.send_modify(|control| control.paused = false);
        self.state = match mem::take(&mut self.state) {
            State::Paused(playback) => State::Playing(playback),
            State::Idle if self.queue.is_empty() => {
                return Err("the queue is empty".to_string());
            }
            State::Idle => {
                self.play_next();
                return Ok(());
            }
            playing @ State::Playing(_) => playing,
        };
        Ok(())
    }

    /// `!song next`. Starts the next track, or stops playback when the queue is empty.
    pub(super) fn next(&mut self) {
        self.stop_current();
        self.play_next();
    }

    /// `!song current`. Always replies (informational command).
    // TODO: artist, source and position in time, once sources give that.
    pub(super) fn current(&self) -> String {
        match &self.state {
            State::Idle => "nothing is playing".to_string(),
            State::Playing(playback) => playback.track.title.clone(),
            State::Paused(playback) => format!("{} (paused)", playback.track.title),
        }
    }

    /// Stops the current track at once, drops its buffered frames and unpauses the client.
    pub(super) fn stop_current(&mut self) {
        if let Some(playback) = self.state.playback() {
            playback.stop.cancel();
        }
        // IDs only go up, so `next_id` is above every stream started so far.
        let drop_before = self.next_id;
        self.control.send_modify(|control| {
            control.drop_before = drop_before;
            control.paused = false;
        });
        self.state = State::Idle;
    }

    /// A track finished decoding, its end may still be in the buffer.
    /// Stale IDs are ignored.
    pub(super) fn finished(&mut self, id: PlaybackId) {
        match &self.state {
            // `stop_current` is not used, the buffered end must play before the next track.
            State::Playing(playback) if playback.id == id => {
                self.state = State::Idle;
                self.play_next();
            }
            // Advancing would cancel the pause, playback continues on `!song resume`.
            State::Paused(playback) if playback.id == id => self.state = State::Idle,
            _ => {}
        }
    }

    /// Starts the first track from the queue that opens. The player must be idle,
    /// and stays idle when the queue runs out.
    fn play_next(&mut self) {
        while let Some(track) = self.queue.pop_next() {
            match self.open(&track.location) {
                Ok(input) => {
                    self.start(input, track);
                    return;
                }
                // Only logged, there is no command to reply to.
                // TODO: tell the chat once unavailable Tidal tracks show up here.
                Err(err) => warn!("skipping {}: {err}", track.title),
            }
        }
    }

    /// Starts `input` through the launcher. The player must be idle.
    fn start(&mut self, input: AudioInput, track: TrackRef) {
        let id = self.new_id();
        let stop = CancellationToken::new();
        let done = self.done_tx.clone();
        // A track that finished while paused leaves the client paused, unpause for the new one.
        self.control.send_modify(|control| control.paused = false);
        self.launcher.launch(
            input,
            id.0,
            stop.clone(),
            // `Err` means the player has stopped, the notification is dropped.
            Box::new(move || {
                let _ = done.blocking_send(id);
            }),
        );
        self.state = State::Playing(Playback { id, stop, track });
    }

    /// Turns a query into a track with the chosen source, or the default one from the config.
    // TODO: move to `sources` once there is a `Source` trait, and off the actor loop
    //   (`Loading` state), since network sources take seconds.
    fn resolve(&self, query: &str, source: Option<SourceKind>) -> Result<TrackRef, String> {
        match source.unwrap_or(self.sources.default) {
            SourceKind::Tidal => todo!(),
            SourceKind::Spotify => todo!(),
            SourceKind::Youtube => todo!(),
            SourceKind::Soundcloud => todo!(),
            SourceKind::Local => {
                local::resolve(self.music_dir()?, query).map_err(|err| err.to_string())
            }
        }
    }

    /// Opens a track right before playing it.
    fn open(&self, location: &Location) -> Result<AudioInput, String> {
        match location {
            Location::Local(path) => {
                local::open(self.music_dir()?, path).map_err(|err| err.to_string())
            }
        }
    }

    /// `None` in the config means the local source is disabled.
    fn music_dir(&self) -> Result<&std::path::Path, String> {
        self.sources
            .local
            .music_dir
            .as_deref()
            .ok_or_else(|| "the local source is disabled".to_string())
    }

    /// Takes the next ID.
    fn new_id(&mut self) -> PlaybackId {
        let id = PlaybackId(self.next_id);
        self.next_id += 1;
        id
    }
}
