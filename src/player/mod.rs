//! Player: the actor that owns the playback state and the queue.
//!
//! Every playback has a `PlaybackId` and a `CancellationToken`. Results with a stale ID are dropped.
//! Handles `!song ...` and `!queue ...` commands.
//!
//! Split like `commands`, one file per command namespace:
//! - `mod.rs`: the actor loop, dispatching commands and chat replies
//! - `launcher`: `Launcher`, the interface to the audio thread
//! - `state`: `State` and `PlaybackId`
//! - `song`: `!song ...` and the playback transitions (start, stop, finish)
//! - `queue`: `!queue ...` and the song queue as a plain data structure

mod launcher;
mod queue;
mod song;
mod state;

pub use launcher::{AudioLauncher, Launcher};

use std::num::NonZeroUsize;

use tokio::sync::{mpsc, watch};

use crate::{
    client::{AudioControl, ClientEvent, ClientRequest, ReplyTo},
    commands::Command,
    config::{Config, SourcesConfig},
};
use queue::Queue;
use state::{PlaybackId, State};

pub struct Player {
    state: State,
    /// Songs after the current one.
    queue: Queue,
    /// Entries per page in `!queue show`.
    page_size: NonZeroUsize,
    /// The ID for the next playback, only ever goes up.
    next_id: u64,
    /// Chat messages from the client, the source of commands.
    events: mpsc::Receiver<ClientEvent>,
    /// Error replies to the chat.
    requests: mpsc::Sender<ClientRequest>,
    /// Starts the audio of a track, `AudioLauncher` outside of tests.
    launcher: Box<dyn Launcher>,
    /// Tells the client to hold the frames (pause) or drop a stopped track's frames.
    control: watch::Sender<AudioControl>,
    /// Audio threads send their ID here when the track ends.
    done_tx: mpsc::Sender<PlaybackId>,
    done_rx: mpsc::Receiver<PlaybackId>,
    sources: SourcesConfig,
}

impl Player {
    pub fn new(
        events: mpsc::Receiver<ClientEvent>,
        requests: mpsc::Sender<ClientRequest>,
        control: watch::Sender<AudioControl>,
        launcher: Box<dyn Launcher>,
        config: &Config,
    ) -> Self {
        let (done_tx, done_rx) = mpsc::channel(8);
        Self {
            state: State::Idle,
            queue: Queue::new(config.queue.max_len),
            page_size: config.queue.show_page_size,
            next_id: 0,
            events,
            requests,
            launcher,
            control,
            done_tx,
            done_rx,
            sources: config.sources.clone(),
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

        let Some(parsed) = Command::parse(&text) else {
            return;
        };
        let reply = match parsed {
            Ok(command) => self.execute(command).await,
            Err(err) => Some(err.to_string()),
        };
        if let Some(text) = reply {
            self.reply(reply_to, text).await;
        }
    }

    /// Runs a command and returns the chat reply, if any.
    /// Success is silent, except for informational commands.
    async fn execute(&mut self, command: Command) -> Option<String> {
        match command {
            Command::Add { query, source, .. } => self.add(&query, source).await.err(),
            Command::Pause => {
                self.pause();
                None
            }
            Command::Resume => self.resume().err(),
            Command::Next => {
                self.next();
                None
            }
            Command::Current => Some(self.current()),
            Command::QueueShow { page } => Some(self.queue_show(page)),
            Command::QueueRemove { position } => self.queue_remove(position).err(),
            Command::QueueClear => {
                self.queue_clear();
                None
            }
            Command::Ping => Some("pong".to_string()),
            Command::Help(text) => Some(text.to_string()),
            // Listed one by one, so a new command does not compile until it is handled.
            // TODO: `!bot summon` belongs to presence, `!bot config` to the config handler.
            Command::Summon
            | Command::ConfigList { .. }
            | Command::ConfigGet { .. }
            | Command::ConfigSet { .. }
            | Command::ConfigReload => None,
        }
    }

    async fn reply(&self, to: ReplyTo, text: String) {
        let request = ClientRequest::SendMessage { to, text };
        // `Err` means the client has stopped, the reply is dropped.
        let _ = self.requests.send(request).await;
    }
}

#[cfg(test)]
mod tests;
