//! Player: the actor that owns the playback state and the queue.
//!
//! Every playback has a `PlaybackId` and a `CancellationToken`. Results with a stale ID are dropped.
//! Handles `!song ...` and `!queue ...` commands.
//!
//! Split like `commands`, one file per command namespace:
//! - `mod.rs`: the actor loop, dispatching commands and chat replies
//! - `state`: `State` and `PlaybackId`
//! - `song`: `!song ...` and the playback transitions (start, stop, finish)
//! - `queue`: `!queue ...` and the song queue as a plain data structure

mod queue;
mod song;
mod state;

use std::path::PathBuf;

use tokio::sync::{mpsc, watch};

use crate::{
    client::{ClientEvent, ClientRequest, ReplyTo},
    commands::Command,
    config::{AudioConfig, SourcesConfig},
};
use state::{PlaybackId, State};

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
    /// Tells the client to hold the frames, the audio thread then waits on the full buffer.
    paused: watch::Sender<bool>,
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
        paused: watch::Sender<bool>,
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
            paused,
            done_tx,
            done_rx,
            ffmpeg_path,
            audio,
            sources,
        }
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
            Some(Ok(Command::Pause)) => self.pause(),
            Some(Ok(Command::Resume)) => self.resume(),
            Some(Ok(Command::Ping)) => self.reply(reply_to, "pong".into()).await,
            Some(Err(err)) => self.reply(reply_to, err.to_string()).await,
            // Normal chat and commands not handled yet.
            // TODO: the rest of !song and !queue.
            Some(Ok(_)) | None => {}
        }
    }

    async fn reply(&self, to: ReplyTo, text: String) {
        let request = ClientRequest::SendMessage { to, text };
        // `Err` means the client is gone, so there is nobody to answer.
        let _ = self.requests.send(request).await;
    }
}
