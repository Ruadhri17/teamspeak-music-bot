//! Player: the actor that owns the playback state and the queue.
//!
//! Every playback has a `PlaybackId` and a `CancellationToken`. Results with a stale ID are dropped.
//! Handles `!song ...` and `!queue ...` commands.

mod queue;

use std::path::PathBuf;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    audio,
    client::{ClientEvent, ClientRequest, ReplyTo},
    commands::Command,
    config::{AudioConfig, SourceKind, SourcesConfig},
    sources::local,
};

/// Identifies one playback, so a late "finished" from an old track can be told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackId(u64);

/// The token and ID live inside the states that have a track, so `Idle` cannot hold a stale one.
// TODO: `Loading` for Tidal, `Departing` for presence.
#[derive(Debug)]
enum State {
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

pub struct Player {
    state: State,
    /// The ID for the next playback, only ever goes up.
    next_id: u64,
    /// Chat messages from the client, the source of commands.
    events: mpsc::Receiver<ClientEvent>,
    /// Error replies to the chat.
    requests: mpsc::Sender<ClientRequest>,
    /// Handed to every audio thread.
    frames: mpsc::Sender<Vec<u8>>,
    /// Audio threads send their ID here when the track ends.
    done_tx: mpsc::Sender<PlaybackId>,
    done_rx: mpsc::Receiver<PlaybackId>,
    ffmpeg_path: PathBuf,
    audio: AudioConfig,
    sources: SourcesConfig,
}

impl Player {
    pub fn new(
        events: mpsc::Receiver<ClientEvent>,
        requests: mpsc::Sender<ClientRequest>,
        frames: mpsc::Sender<Vec<u8>>,
        ffmpeg_path: PathBuf,
        audio: AudioConfig,
        sources: SourcesConfig,
    ) -> Self {
        // One message per finished track, a few slots are plenty.
        let (done_tx, done_rx) = mpsc::channel(8);
        Self {
            state: State::Idle,
            next_id: 0,
            events,
            requests,
            frames,
            done_tx,
            done_rx,
            ffmpeg_path,
            audio,
            sources,
        }
    }

    /// Takes the next ID, so no two playbacks ever share one.
    fn new_id(&mut self) -> PlaybackId {
        let id = PlaybackId(self.next_id);
        self.next_id += 1;
        id
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                event = self.events.recv() => match event {
                    Some(event) => self.handle_event(event).await,
                    None => break, // client is gone
                },
                Some(id) = self.done_rx.recv() => self.finished(id),
            }
        }
    }

    async fn handle_event(&mut self, event: ClientEvent) {
        let ClientEvent::Message { reply_to, text, .. } = event;

        match Command::parse(&text) {
            Some(Ok(Command::Add { query, source, .. })) => {
                if let Err(message) = self.add(&query, source) {
                    self.reply(reply_to, message).await;
                }
            }
            Some(Ok(Command::Ping)) => self.reply(reply_to, "pong".into()).await,
            Some(Err(err)) => self.reply(reply_to, err.to_string()).await,
            // Normal chat and commands not handled yet.
            // TODO: the rest of !song and !queue.
            Some(Ok(_)) | None => {}
        }
    }

    /// `Err` is the message for the chat.
    fn add(&mut self, query: &str, source: Option<SourceKind>) -> Result<(), String> {
        let input = match source.unwrap_or(self.sources.default) {
            SourceKind::Tidal => todo!(),
            SourceKind::Spotify => todo!(),
            SourceKind::Youtube => todo!(),
            SourceKind::Soundcloud => todo!(),
            SourceKind::Local => {
                // `None` when the local source is disabled in the config.
                let Some(music_dir) = &self.sources.local.music_dir else {
                    return Err("the local source is disabled".into());
                };
                // Opened before stopping, so a wrong path leaves the current track playing.
                local::open(music_dir, query).map_err(|err| err.to_string())?
            }
        };

        // TODO: append to the queue instead of replacing the current track.
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
        Ok(())
    }

    async fn reply(&self, to: ReplyTo, text: String) {
        let request = ClientRequest::SendMessage { to, text };
        // `Err` means the client is gone, so there is nobody to answer.
        let _ = self.requests.send(request).await;
    }

    /// Stops the current track, if any, and leaves the player idle.
    fn stop_current(&mut self) {
        if let State::Playing { stop, .. } | State::Paused { stop, .. } = &self.state {
            stop.cancel();
        }
        self.state = State::Idle;
    }

    /// A track ended on its own or after `stop`. Only the current one changes the state.
    fn finished(&mut self, id: PlaybackId) {
        // A stale ID means this track was already replaced, nothing to do.
        if let State::Playing { id: current, .. } = self.state
            && current == id
        {
            self.state = State::Idle;
        }
    }
}
