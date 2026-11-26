//! The playback transitions shared by the song and queue operations: queueing, starting,
//! pausing, stopping and finishing tracks, the sounds of events, and the volume.

use std::{fs::File, mem};

use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::{
    Enqueued, Notice, Player, PlayerError,
    state::{Ended, Kind, Playback, PlaybackId, State},
};
use crate::{
    audio::{AudioError, AudioInput},
    config::Volume,
    sources::{
        Location, TrackRef, local, music_dir,
        youtube::{self, YoutubeError},
    },
};

impl Player {
    /// Queues the tracks until the queue is full. A track starts when nothing plays,
    /// a paused track stays paused.
    pub fn enqueue(&mut self, tracks: Vec<TrackRef>) -> Enqueued {
        let mut added = 0;
        for track in tracks {
            if let Err(full) = self.queue.push(track) {
                return Enqueued {
                    added,
                    full: Some(full),
                };
            }
            added += 1;
            // Started one by one, so the first track leaves the queue before the next ones
            // are counted against its limit.
            if let State::Idle = self.state {
                self.play_next();
            }
        }
        Enqueued { added, full: None }
    }

    /// Pauses a playing track, otherwise does nothing. The sound of an event is not paused.
    pub fn pause(&mut self) {
        self.state = match mem::take(&mut self.state) {
            State::Playing(playback) if playback.kind == Kind::Track => {
                self.control.send_modify(|control| control.paused = true);
                State::Paused(playback)
            }
            other => other,
        };
    }

    /// Resumes a paused track, or starts the queue when idle.
    /// Unpauses the client even when idle, a track that finished while paused
    /// may still have frames in the buffer.
    pub fn resume(&mut self) -> Result<(), PlayerError> {
        self.control.send_modify(|control| control.paused = false);
        self.state = match mem::take(&mut self.state) {
            State::Paused(playback) => State::Playing(playback),
            State::Idle if self.queue.is_empty() => return Err(PlayerError::QueueEmpty),
            State::Idle => {
                self.play_next();
                return Ok(());
            }
            playing @ State::Playing(_) => playing,
        };
        Ok(())
    }

    /// Starts the next track, or stops playback when the queue is empty.
    pub fn next(&mut self) {
        self.stop_current();
        self.play_next();
    }

    /// Starts the queue when nothing plays. For the bot, after the sound of an event that
    /// the queue waited for.
    pub fn start_queue(&mut self) {
        if let State::Idle = self.state {
            self.play_next();
        }
    }

    /// Plays the sound of an event outside the queue, in place of whatever plays or is
    /// paused. `Err` is for the log: the file or link could not be opened, nothing changed.
    pub fn play_sound(&mut self, sound: TrackRef) -> Result<(), String> {
        let input = self.open(&sound.location)?;
        self.stop_current();
        self.start(input, sound, Kind::Sound);
        Ok(())
    }

    /// Stops the sound of an event, a track keeps playing.
    pub fn stop_sound(&mut self) {
        if matches!(&self.state, State::Playing(playback) if playback.kind == Kind::Sound) {
            self.stop_current();
        }
    }

    /// Stops the current track at once, drops its buffered frames and unpauses the client.
    pub(super) fn stop_current(&mut self) {
        if let Some(playback) = self.state.playback() {
            playback.stop.cancel();
            if playback.kind == Kind::Sound {
                self.notices.push(Notice::SoundInterrupted);
            }
        }
        // IDs only go up, so `next_id` is above every stream started so far: the client
        // drops their buffered frames and plays only the streams started from now on.
        let min_stream_id = self.next_id;
        self.control.send_modify(|control| {
            control.min_stream_id = min_stream_id;
            control.paused = false;
        });
        self.state = State::Idle;
    }

    /// A track finished decoding, its end may still be in the buffer.
    /// Stale IDs are ignored. A failure of the current track becomes a notice.
    /// The end of a sound is only reported, the bot decides whether the queue goes on.
    pub fn finished(&mut self, Ended { id, failure }: Ended) {
        if let State::Playing(playback) = &self.state
            && playback.kind == Kind::Sound
            && playback.id == id
        {
            if let Some(reason) = failure {
                warn!("the sound {} failed: {reason}", playback.track.title);
            }
            self.state = State::Idle;
            self.notices.push(Notice::SoundEnded);
            return;
        }
        if let (Some(playback), Some(reason)) = (self.state.playback(), failure)
            && playback.id == id
        {
            self.notices.push(Notice::CouldNotPlay {
                title: playback.track.title.clone(),
                reason,
            });
        }
        match &self.state {
            // `stop_current` is not used, the buffered end must play before the next track.
            State::Playing(playback) if playback.id == id => {
                self.state = State::Idle;
                self.play_next();
            }
            // Advancing would cancel the pause, playback continues on `resume`.
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
                    self.start(input, track, Kind::Track);
                    return;
                }
                Err(reason) => {
                    warn!("skipping {}: {reason}", track.title);
                    self.notices.push(Notice::CouldNotPlay {
                        title: track.title,
                        reason,
                    });
                }
            }
        }
    }

    /// Starts `input` through the launcher. The player must be idle.
    fn start(&mut self, input: AudioInput, track: TrackRef, kind: Kind) {
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
        self.state = State::Playing(Playback {
            id,
            stop,
            track,
            kind,
        });
    }

    /// The volume the client applies right now.
    pub fn volume(&self) -> Volume {
        self.control.borrow().volume
    }

    /// Heard in the next frame, the client applies it right before sending.
    pub fn set_volume(&mut self, volume: Volume) -> Result<(), PlayerError> {
        if volume > self.max_volume {
            return Err(PlayerError::VolumeTooHigh(self.max_volume.get()));
        }
        self.control.send_modify(|control| control.volume = volume);
        Ok(())
    }

    /// Opens a track right before playing it. `Err` is safe to show on the chat.
    fn open(&self, location: &Location) -> Result<AudioInput, String> {
        match location {
            Location::Local(path) => {
                local::open(music_dir(&self.sources)?, path).map_err(|err| err.to_string())
            }
            Location::Youtube(id) => {
                youtube::open(&self.sources.youtube, id).map_err(|err| err.to_string())
            }
            // Only event sounds from the config are files, their errors go to the log, where
            // the path helps the admin.
            Location::File(path) => File::open(path)
                .map(AudioInput::File)
                .map_err(|err| format!("cannot open {}: {err}", path.display())),
        }
    }

    /// Takes the next ID.
    fn new_id(&mut self) -> PlaybackId {
        let id = PlaybackId(self.next_id);
        self.next_id += 1;
        id
    }
}

/// `AudioError` carries ffmpeg output and paths, so only the known messages of the sources
/// reach the chat (e.g. "this video is age-restricted"), anything else is generic.
pub(crate) fn chat_reason(err: &AudioError) -> String {
    if let AudioError::Input(err) = err
        && let Some(err) = err
            .get_ref()
            .and_then(|err| err.downcast_ref::<YoutubeError>())
    {
        return err.to_string();
    }
    "playback failed".to_string()
}
