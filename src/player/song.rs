//! `!song ...` commands and the playback transitions shared with `queue`:
//! starting, stopping and finishing tracks.

use std::mem;

use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::{
    Player,
    state::{Ended, Playback, PlaybackId, State},
};
use crate::{
    audio::{AudioError, AudioInput},
    chat,
    config::{SourceKind, Volume},
    sources::{
        Location, QueryType, TrackRef, local,
        youtube::{self, YoutubeError},
    },
};

impl Player {
    /// `!song add`. Appends to the queue and starts it when nothing plays.
    /// A paused track stays paused. `Err` is the message for the chat.
    pub(super) async fn add(
        &mut self,
        query: &QueryType,
        source: Option<SourceKind>,
    ) -> Result<(), String> {
        let track = self.resolve(query, source).await?;
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
            State::Playing(playback) => chat::escape(&playback.track.title).into_owned(),
            State::Paused(playback) => {
                format!("{} (paused)", chat::escape(&playback.track.title))
            }
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
    /// Stale IDs are ignored. A failure of the current track goes to the channel.
    pub(super) fn finished(&mut self, Ended { id, failure }: Ended) {
        if let (Some(playback), Some(reason)) = (self.state.playback(), failure)
            && playback.id == id
        {
            self.notices
                .push(could_not_play(&playback.track.title, &reason));
        }
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
                Err(err) => {
                    warn!("skipping {}: {err}", track.title);
                    self.notices.push(could_not_play(&track.title, &err));
                }
            }
        }
    }

    /// Starts `input` through the launcher. The player must be idle.
    fn start(&mut self, input: AudioInput, track: TrackRef) {
        let id = self.new_id();
        let stop = CancellationToken::new();
        let done = self.done_tx.clone();
        let title = track.title.clone();
        // A track that finished while paused leaves the client paused, unpause for the new one.
        self.control.send_modify(|control| control.paused = false);
        self.launcher.launch(
            input,
            id.0,
            stop.clone(),
            Box::new(move |result| {
                // A failed track ends like a finished one and the queue moves on.
                let failure = result.err().map(|err| {
                    warn!("playback of {title} failed: {err}");
                    chat_reason(&err)
                });
                // `Err` means the player has stopped, the notification is dropped.
                let _ = done.blocking_send(Ended { id, failure });
            }),
        );
        self.state = State::Playing(Playback { id, stop, track });
    }

    /// The volume the client applies right now.
    pub(super) fn volume(&self) -> Volume {
        self.control.borrow().volume
    }

    /// `!bot volume <n>`. Heard in the next frame, the client applies it right before sending.
    pub(super) fn set_volume(&mut self, volume: Volume) -> Result<(), String> {
        if volume > self.max_volume {
            return Err(format!("the highest volume is {}", self.max_volume.get()));
        }
        self.control.send_modify(|control| control.volume = volume);
        Ok(())
    }

    /// Turns a query into a track with the chosen source, or the default one from the config.
    // TODO: move to `sources` once there is a `Source` trait, and off the actor loop
    //   (`Loading` state), since network sources take seconds.
    async fn resolve(
        &self,
        query: &QueryType,
        source: Option<SourceKind>,
    ) -> Result<TrackRef, String> {
        match source.unwrap_or(self.sources.default) {
            SourceKind::Tidal | SourceKind::Spotify | SourceKind::Soundcloud => {
                Err("this source is not supported yet".to_string())
            }
            SourceKind::Youtube => youtube::resolve(&self.sources.youtube, query)
                .await
                .map_err(|err| err.to_string()),
            SourceKind::Local => match query {
                QueryType::Phrase(path) => {
                    local::resolve(self.music_dir()?, path).map_err(|err| err.to_string())
                }
                QueryType::Link(_) => Err("the local source takes a path, not a link".to_string()),
            },
        }
    }

    /// Opens a track right before playing it.
    fn open(&self, location: &Location) -> Result<AudioInput, String> {
        match location {
            Location::Local(path) => {
                local::open(self.music_dir()?, path).map_err(|err| err.to_string())
            }
            Location::Youtube(id) => {
                youtube::open(&self.sources.youtube, id).map_err(|err| err.to_string())
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

/// The title comes from outside the bot, so it is escaped.
fn could_not_play(title: &str, reason: &str) -> String {
    format!("could not play {}: {reason}", chat::escape(title))
}

/// `AudioError` carries ffmpeg output and paths, so only the known messages of the sources
/// reach the chat (e.g. "this video is age-restricted"), anything else is generic.
pub(super) fn chat_reason(err: &AudioError) -> String {
    if let AudioError::Input(err) = err
        && let Some(err) = err
            .get_ref()
            .and_then(|err| err.downcast_ref::<YoutubeError>())
    {
        return err.to_string();
    }
    "playback failed".to_string()
}
