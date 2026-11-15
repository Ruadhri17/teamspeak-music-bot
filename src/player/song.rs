//! `!song ...` commands and the playback transitions they share with `queue`:
//! starting, stopping and finishing tracks. Everything that touches the state,
//! its token and its ID lives here.

use tokio_util::sync::CancellationToken;

use super::{
    Player,
    state::{PlaybackId, State},
};
use crate::{
    audio::{self, AudioInput},
    config::SourceKind,
    sources::local,
};

impl Player {
    /// `!song add`. `Err` is the message for the chat.
    pub(super) fn add(&mut self, query: &str, source: Option<SourceKind>) -> Result<(), String> {
        // Opened before stopping, so a wrong path leaves the current track playing.
        let input = self.open(query, source)?;
        // TODO: append to the queue instead of replacing the current track.
        self.start(input);
        Ok(())
    }

    /// Starts `input` as the current track, stopping whatever played before.
    pub(super) fn start(&mut self, input: AudioInput) {
        self.stop_current();
        let id = self.new_id();
        let stop = CancellationToken::new();
        let done = self.done_tx.clone();
        audio::play(
            self.ffmpeg_path.clone(),
            input,
            &self.audio,
            self.frames.clone(),
            stop.clone(),
            // `Err` means the player is gone, nobody waits for the answer.
            move || {
                let _ = done.blocking_send(id);
            },
        );
        self.state = State::Playing { id, stop };
    }

    /// Stops the current track, if any, and leaves the player idle.
    /// The flag is cleared even when idle, since `finished` can leave a paused track `Idle`
    /// with the flag still set, and the next track would stay silent.
    // TODO: drop the frames still buffered in `frames`, now a few seconds of the old track
    //   play before the new one (`!song add`, `!song next`).
    pub(super) fn stop_current(&mut self) {
        if let State::Playing { stop, .. } | State::Paused { stop, .. } = &self.state {
            stop.cancel();
        }
        self.paused.send_replace(false);
        self.state = State::Idle;
    }

    /// `!song pause`. Pauses a playing track, otherwise does nothing.
    /// The client holds the frames, so the audio thread waits on the full buffer
    /// and the track resumes exactly where it stopped.
    pub(super) fn pause(&mut self) {
        if let State::Playing { id, stop } = &self.state {
            self.paused.send_replace(true);
            self.state = State::Paused {
                id: *id,
                stop: stop.clone(),
            }
        }
    }

    /// `!song resume`. Clears the flag even without a paused track, since a paused track
    /// may have finished decoding and gone `Idle` with its last frames still buffered.
    pub(super) fn resume(&mut self) {
        if let State::Paused { id, stop } = &self.state {
            self.state = State::Playing {
                id: *id,
                stop: stop.clone(),
            }
        }

        self.paused.send_replace(false);
    }

    /// A track ended on its own or after `stop`. Only the current one changes the state.
    pub(super) fn finished(&mut self, id: PlaybackId) {
        // A stale ID means this track was already replaced, nothing to do.
        if let State::Playing { id: current, .. } | State::Paused { id: current, .. } = self.state
            && current == id
        {
            self.state = State::Idle;
        }
    }

    /// Turns a query into audio with the chosen source, or the default one from the config.
    // TODO: move to `sources` once there is a `Source` trait, and off the actor loop
    //   (`Loading` state), since network sources take seconds.
    fn open(&self, query: &str, source: Option<SourceKind>) -> Result<AudioInput, String> {
        match source.unwrap_or(self.sources.default) {
            SourceKind::Tidal => todo!(),
            SourceKind::Spotify => todo!(),
            SourceKind::Youtube => todo!(),
            SourceKind::Soundcloud => todo!(),
            SourceKind::Local => {
                // `None` when the local source is disabled in the config.
                let Some(music_dir) = &self.sources.local.music_dir else {
                    return Err("the local source is disabled".into());
                };
                local::open(music_dir, query).map_err(|err| err.to_string())
            }
        }
    }

    /// Takes the next ID, so no two playbacks ever share one.
    fn new_id(&mut self) -> PlaybackId {
        let id = PlaybackId(self.next_id);
        self.next_id += 1;
        id
    }
}
