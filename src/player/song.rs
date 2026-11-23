//! `!song ...` commands and the playback transitions shared with `queue`:
//! starting, stopping and finishing tracks.

use std::mem;

use tokio::task::{Id, JoinError};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::{
    Player,
    state::{Ended, Pending, Playback, PlaybackId, State},
};
use crate::{
    audio::{AudioError, AudioInput},
    chat,
    client::ReplyTo,
    config::{SourceKind, SourcesConfig, Volume},
    sources::{
        Found, Location, QueryType, TrackRef, local,
        youtube::{self, YoutubeError},
    },
};

impl Player {
    /// `!song add`. Starts the lookup, `lookup_done` queues the track in the order of the commands.
    pub(super) fn add(&mut self, query: QueryType, source: Option<SourceKind>, reply_to: ReplyTo) {
        let sources = self.sources.clone();
        let task = self.lookups.spawn(resolve(sources, query, source)).id();
        self.pending.push_back(Pending {
            task,
            reply_to,
            result: None,
        });
    }

    /// Stores the result of a lookup, then queues every finished lookup at the front of
    /// `pending`. A track starts when nothing plays, a paused track stays paused.
    /// Errors and the summary of a playlist go to the place the command came from.
    pub(super) fn lookup_done(&mut self, joined: Result<(Id, Result<Found, String>), JoinError>) {
        let (task, result) = match joined {
            Ok(done) => done,
            // Aborted by `!queue clear`, which also removed the entry.
            Err(err) if err.is_cancelled() => return,
            Err(err) => {
                warn!("lookup task panicked: {err}");
                (err.id(), Err("the lookup failed".to_string()))
            }
        };
        if let Some(entry) = self.pending.iter_mut().find(|entry| entry.task == task) {
            entry.result = Some(result);
        }

        while let Some(entry) = self.pending.pop_front_if(|entry| entry.result.is_some()) {
            let found = match entry.result.expect("only finished entries are taken") {
                Ok(found) => found,
                Err(text) => {
                    self.outbox.push((entry.reply_to, text));
                    continue;
                }
            };
            if let Some(text) = self.enqueue_found(found) {
                self.outbox.push((entry.reply_to, text));
            }
        }
    }

    /// Queues the tracks until the queue is full. Returns the message for the sender:
    /// a full queue, or the summary of a playlist. A single track that fits is silent.
    fn enqueue_found(&mut self, Found { tracks, playlist }: Found) -> Option<String> {
        let count = tracks.len();
        let mut added = 0;
        let mut full = None;
        for track in tracks {
            if let Err(err) = self.queue.push(track) {
                full = Some(err);
                break;
            }
            added += 1;
            if let State::Idle = self.state {
                self.play_next();
            }
        }

        let total = playlist
            .as_ref()
            .and_then(|playlist| playlist.total)
            .map_or(count, |total| total.max(count));
        let summary = || match &playlist {
            Some(playlist) => added_from(added, total, &playlist.title),
            None => format!("added {added} of {total} songs"),
        };
        match full {
            Some(full) if added == 0 => Some(full.to_string()),
            Some(full) => Some(format!("{full}, {}", summary())),
            None => playlist.is_some().then(summary),
        }
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
    /// Stale IDs are ignored. A failure of the current track goes to the channel.
    pub(super) fn finished(&mut self, Ended { id, failure }: Ended) {
        if let (Some(playback), Some(reason)) = (self.state.playback(), failure)
            && playback.id == id
        {
            self.outbox.push((
                ReplyTo::Channel,
                could_not_play(&playback.track.title, &reason),
            ));
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
                    self.outbox
                        .push((ReplyTo::Channel, could_not_play(&track.title, &err)));
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

    /// Opens a track right before playing it.
    fn open(&self, location: &Location) -> Result<AudioInput, String> {
        match location {
            Location::Local(path) => {
                local::open(music_dir(&self.sources)?, path).map_err(|err| err.to_string())
            }
            Location::Youtube(id) => {
                youtube::open(&self.sources.youtube, id).map_err(|err| err.to_string())
            }
        }
    }

    /// Takes the next ID.
    fn new_id(&mut self) -> PlaybackId {
        let id = PlaybackId(self.next_id);
        self.next_id += 1;
        id
    }
}

/// Turns a query into a track with the chosen source, or the default one from the config.
async fn resolve(
    sources: SourcesConfig,
    query: QueryType,
    source: Option<SourceKind>,
) -> Result<Found, String> {
    match source.unwrap_or(sources.default) {
        SourceKind::Tidal | SourceKind::Spotify | SourceKind::Soundcloud => {
            Err("this source is not supported yet".to_string())
        }
        SourceKind::Youtube => youtube::resolve(&sources.youtube, &query, sources.max_playlist_len)
            .await
            .map_err(|err| err.to_string()),
        SourceKind::Local => match query {
            QueryType::Phrase(path) => {
                let track =
                    local::resolve(music_dir(&sources)?, &path).map_err(|err| err.to_string())?;
                Ok(Found::track(track))
            }
            QueryType::Link(_) => Err("the local source takes a path, not a link".to_string()),
        },
    }
}

/// `None` in the config means the local source is disabled.
fn music_dir(sources: &SourcesConfig) -> Result<&std::path::Path, String> {
    sources
        .local
        .music_dir
        .as_deref()
        .ok_or_else(|| "the local source is disabled".to_string())
}

/// `added 3 songs from X`, or `added 100 of 182 songs from X` when part of the playlist
/// was left out. The title comes from outside the bot, so it is escaped.
fn added_from(added: usize, total: usize, title: &str) -> String {
    let songs = if total == 1 { "song" } else { "songs" };
    let title = chat::escape(title);
    if added < total {
        format!("added {added} of {total} {songs} from {title}")
    } else {
        format!("added {added} {songs} from {title}")
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
