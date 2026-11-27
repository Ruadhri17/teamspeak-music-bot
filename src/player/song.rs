//! The playback transitions shared by the song and queue operations: queueing, starting,
//! pausing, stopping and finishing tracks, the sounds of events, and the volume.

use std::{fs::File, mem};

use tracing::warn;

use super::{
    Enqueued, Notice, Player, PlayerError,
    state::{Ended, Kind, Playback, PlaybackId, State},
};
use crate::{
    audio::{AudioError, AudioInput},
    config::Volume,
    process::Stopper,
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
            // are counted against its limit. A pause holds, the new track waits for `resume`.
            if self.can_start() {
                self.play_next();
            }
        }
        Enqueued { added, full: None }
    }

    /// Pauses a playing track, also when only its end is left in the buffer. Otherwise
    /// does nothing, the sound of an event is not paused.
    pub fn pause(&mut self) {
        self.state = match mem::take(&mut self.state) {
            State::Playing(playback) if playback.kind == Kind::Track => {
                self.control.send_modify(|control| control.paused = true);
                State::Paused(playback)
            }
            State::Idle if !self.tails.is_empty() => {
                self.control.send_modify(|control| control.paused = true);
                State::Idle
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
            State::Idle if self.queue.is_empty() && self.tails.is_empty() => {
                return Err(PlayerError::QueueEmpty);
            }
            // The end of a track that finished while paused plays on.
            State::Idle if self.queue.is_empty() => State::Idle,
            State::Idle => {
                self.play_next();
                return Ok(());
            }
            playing @ State::Playing(_) => playing,
        };
        Ok(())
    }

    /// Skips the track heard now: starts the next one, or stops playback when the queue is
    /// empty.
    pub fn next(&mut self) {
        if self.tails.is_empty() {
            self.stop_current();
            self.play_next();
            return;
        }
        // The end of a decoded track is heard. Only its frames go, the track behind it
        // keeps what it decoded already.
        self.tails.pop_front();
        let keep_from = self
            .tails
            .front()
            .or(self.state.playback())
            .map_or(self.next_id, |playback| playback.id.0);
        self.control.send_modify(|control| {
            control.min_stream_id = keep_from;
            control.paused = false;
        });
        self.state = match mem::take(&mut self.state) {
            State::Paused(playback) => State::Playing(playback),
            other => other,
        };
        if self.tails.is_empty() && matches!(self.state, State::Idle) {
            self.play_next();
        } else if self.track_heard() {
            self.notices.push(Notice::SongStarted);
        }
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
        let stop = Stopper::new();
        let input = self.open(&sound.location, &stop)?;
        self.stop_current();
        self.start(input, sound, Kind::Sound, stop);
        Ok(())
    }

    /// Stops the sound of an event, a track keeps playing.
    pub fn stop_sound(&mut self) {
        if matches!(&self.state, State::Playing(playback) if playback.kind == Kind::Sound) {
            self.stop_current();
        }
    }

    /// Stops the current track at once (its processes too), drops its buffered frames and
    /// unpauses the client.
    pub(super) fn stop_current(&mut self) {
        if let Some(playback) = self.state.playback() {
            playback.stop.stop();
            if playback.kind == Kind::Sound {
                self.notices.push(Notice::SoundInterrupted);
            }
        }
        self.drop_buffered();
        self.state = State::Idle;
    }

    /// Drops every buffered frame and the tails they belonged to, and unpauses the client.
    fn drop_buffered(&mut self) {
        // IDs only go up, so `next_id` is above every stream started so far: the client
        // drops their buffered frames and plays only the streams started from now on.
        // The ends of decoded tracks go with them.
        self.tails.clear();
        self.played_early = None;
        let min_stream_id = self.next_id;
        self.control.send_modify(|control| {
            control.min_stream_id = min_stream_id;
            control.paused = false;
        });
    }

    /// A track finished decoding, its end is still in the buffer: it becomes a tail, heard
    /// before the next track, which starts decoding now.
    /// Stale IDs are ignored. A failure of the current track becomes a notice, a failed
    /// track has no end to wait for.
    /// A sound that decoded to its end waits for `played`, its end is still to be heard.
    /// A failed sound ends at once, the bot decides whether the queue goes on.
    pub fn finished(
        &mut self,
        Ended {
            id,
            failure,
            end_frame,
        }: Ended,
    ) {
        if let State::Playing(playback) = &self.state
            && playback.kind == Kind::Sound
            && playback.id == id
        {
            if let Some(reason) = &failure {
                warn!("the sound {} failed: {reason}", playback.track.title);
            }
            // What it decoded plays to its end and `played` ends it, like a whole sound.
            if !end_frame {
                self.drop_buffered();
                self.state = State::Idle;
                self.notices.push(Notice::SoundEnded);
            }
            return;
        }
        // Heard to its end already, there is no end left to wait for.
        let played = self.played_early.take_if(|early| *early == id).is_some();
        if let (Some(playback), Some(reason)) = (self.state.playback(), failure)
            && playback.id == id
        {
            self.notices.push(Notice::CouldNotPlay {
                title: playback.track.title.clone(),
                reason,
            });
        }
        // Without its end in the buffer nothing would ever report the rest as heard, so it
        // is dropped and the queue moves on at once.
        if !end_frame {
            if self
                .state
                .playback()
                .is_some_and(|playback| playback.id == id)
            {
                let playing = matches!(self.state, State::Playing(_));
                self.drop_buffered();
                self.state = State::Idle;
                if playing {
                    self.play_next();
                } else {
                    // A pause stays a pause: the queue waits for `resume`.
                    self.control.send_modify(|control| control.paused = true);
                }
            }
            return;
        }
        // A failed track keeps its tail too: what it decoded is heard before the next one.
        let tail = !played;
        match mem::take(&mut self.state) {
            // `stop_current` is not used, the buffered end must play before the next track.
            State::Playing(playback) if playback.id == id => {
                if tail {
                    self.tails.push_back(playback);
                }
                self.play_next();
            }
            // Advancing would cancel the pause, playback continues on `resume`.
            State::Paused(playback) if playback.id == id => {
                if tail {
                    self.tails.push_back(playback);
                }
            }
            other => self.state = other,
        }
    }

    /// Every stream up to `heard` was heard to its end. Streams end in order, and a stopped
    /// one never sends its end, so one report covers the streams before it too. Ends the
    /// sound of an event when it is still the current one. For tracks, their tails are gone
    /// and the next track is heard, and a current track that is heard already must not wait
    /// for its end in `finished`.
    pub fn played(&mut self, heard: u64) {
        if let State::Playing(playback) = &self.state
            && playback.kind == Kind::Sound
            && playback.id.0 <= heard
        {
            self.state = State::Idle;
            self.notices.push(Notice::SoundEnded);
            return;
        }
        let before = self.tails.len();
        while self.tails.front().is_some_and(|tail| tail.id.0 <= heard) {
            self.tails.pop_front();
        }
        // The end came before `finished`, which then must not wait for it.
        if let Some(playback) = self.state.playback()
            && playback.kind == Kind::Track
            && playback.id.0 <= heard
        {
            self.played_early = Some(playback.id);
        }
        if self.tails.len() < before && self.track_heard() {
            self.notices.push(Notice::SongStarted);
        }
    }

    /// Starts the first track from the queue that opens. The player must be idle,
    /// and stays idle when the queue runs out.
    fn play_next(&mut self) {
        while let Some(track) = self.queue.pop_next() {
            let stop = Stopper::new();
            match self.open(&track.location, &stop) {
                Ok(input) => {
                    self.start(input, track, Kind::Track, stop);
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

    /// Starts `input` through the launcher. The player must be idle. `stop` is the one
    /// `input` was opened with, so stopping the playback stops its processes too.
    fn start(&mut self, input: AudioInput, track: TrackRef, kind: Kind, stop: Stopper) {
        let id = self.new_id();
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
                let end_frame = !matches!(result, Err(AudioError::Panicked));
                let failure = result.err().map(|err| {
                    warn!("playback of {title} failed: {err}");
                    chat_reason(&err)
                });
                // `Err` means the player has stopped, the notification is dropped.
                let _ = done.blocking_send(Ended {
                    id,
                    failure,
                    end_frame,
                });
            }),
        );
        self.state = State::Playing(Playback {
            id,
            stop,
            track,
            kind,
        });
        // Behind the end of a decoded track it is heard later, `played` reports it then.
        if kind == Kind::Track && self.tails.is_empty() {
            self.notices.push(Notice::SongStarted);
        }
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

    /// Opens a track right before playing it. Processes it starts are stopped by `stop`.
    /// `Err` is safe to show on the chat.
    fn open(&self, location: &Location, stop: &Stopper) -> Result<AudioInput, String> {
        match location {
            Location::Local(path) => {
                local::open(music_dir(&self.sources)?, path).map_err(|err| err.to_string())
            }
            Location::Youtube(id) => {
                youtube::open(&self.sources.youtube, id, stop).map_err(|err| err.to_string())
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
