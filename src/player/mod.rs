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

use std::{collections::VecDeque, num::NonZeroUsize};

use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};
use tracing::debug;

use crate::{
    client::{AudioControl, ClientEvent, ClientRequest, ReplyTo, Sender},
    commands::Command,
    config::{Config, SourcesConfig, Volume},
    permissions::{self, Role},
    sources::Found,
};
use queue::Queue;
use state::{Ended, Pending, State};

pub struct Player {
    state: State,
    /// Songs after the current one.
    queue: Queue,
    /// Entries per page in `!queue show`.
    page_size: NonZeroUsize,
    /// The ID for the next playback, only ever goes up.
    next_id: u64,
    /// Lookups started by `!song add`, matched to `pending` by task ID.
    lookups: JoinSet<Result<Found, String>>,
    /// The lookups in the order of the commands, a finished one waits for those before it.
    pending: VecDeque<Pending>,
    /// Chat messages from the client, the source of commands.
    events: mpsc::Receiver<ClientEvent>,
    /// Error replies to the chat.
    requests: mpsc::Sender<ClientRequest>,
    /// Starts the audio of a track, `AudioLauncher` outside of tests.
    launcher: Box<dyn Launcher>,
    /// Tells the client to hold the frames (pause) or drop a stopped track's frames.
    control: watch::Sender<AudioControl>,
    /// Audio threads report here when the track ends.
    done_tx: mpsc::Sender<Ended>,
    done_rx: mpsc::Receiver<Ended>,
    /// Messages that are not direct replies to a command, sent after the current event is handled.
    outbox: Vec<ClientRequest>,
    sources: SourcesConfig,
    /// `!bot volume` above this is refused.
    max_volume: Volume,
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
        // The player owns the volume, the client only reads it from `control`.
        control.send_modify(|control| control.volume = config.audio.default_volume);
        Self {
            state: State::Idle,
            queue: Queue::new(config.queue.max_len),
            page_size: config.queue.show_page_size,
            next_id: 0,
            lookups: JoinSet::new(),
            pending: VecDeque::new(),
            events,
            requests,
            launcher,
            control,
            done_tx,
            done_rx,
            outbox: Vec::new(),
            sources: config.sources.clone(),
            max_volume: config.audio.max_volume,
        }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                event = self.events.recv() => match event {
                    Some(event) => self.handle_event(event).await,
                    None => break, // client is gone
                },
                Some(ended) = self.done_rx.recv() => self.finished(ended),
                Some(joined) = self.lookups.join_next_with_id() => self.lookup_done(joined),
            }
            self.send_outbox().await;
        }
    }

    /// Sends the outbox to the client, in order.
    async fn send_outbox(&mut self) {
        for request in std::mem::take(&mut self.outbox) {
            // `Err` means the client has stopped, the request is dropped.
            let _ = self.requests.send(request).await;
        }
    }

    /// Queues a chat message for `send_outbox`.
    fn say(&mut self, to: ReplyTo, text: String) {
        self.outbox.push(ClientRequest::SendMessage { to, text });
    }

    async fn handle_event(&mut self, event: ClientEvent) {
        let ClientEvent::Message {
            reply_to,
            sender,
            text,
        } = event;
        if let Some(text) = self.command(&text, reply_to, &sender) {
            self.reply(reply_to, text).await;
        }
    }

    /// Parses a chat message, checks the sender's permissions and runs the command.
    /// Returns the chat reply, `None` for text that is not a command.
    fn command(&mut self, text: &str, reply_to: ReplyTo, sender: &Sender) -> Option<String> {
        let command = match Command::parse(text)? {
            Ok(command) => command,
            Err(err) => return Some(err.to_string()),
        };
        if let Err(denied) = permissions::check(&command, sender.role, sender.in_bot_channel) {
            debug!("command from {} refused: {denied}", sender.name);
            return Some(denied.to_string());
        }
        self.execute(command, reply_to, sender)
    }

    /// Runs a command and returns the chat reply, if any.
    /// Success is silent, except for informational commands.
    fn execute(&mut self, command: Command, reply_to: ReplyTo, sender: &Sender) -> Option<String> {
        match command {
            Command::Add { query, source, .. } => {
                self.add(query, source, reply_to);
                None
            }
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
            Command::Volume(None) => Some(format!("volume: {}", self.volume().get())),
            Command::Volume(Some(volume)) => self.set_volume(volume).err(),
            Command::QueueShow { page } => Some(self.queue_show(page)),
            Command::QueueRemove { position } => self.queue_remove(position).err(),
            Command::QueueClear => {
                self.queue_clear();
                None
            }
            Command::Ping => Some("pong".to_string()),
            Command::Help(text) => Some(text.to_string()),
            // The client knows the channels, it decides and answers.
            Command::Summon => {
                self.outbox.push(ClientRequest::Summon {
                    sender_id: sender.id,
                    reply_to,
                    admin: sender.role == Role::Admin,
                });
                None
            }
            // Listed one by one, so a new command does not compile until it is handled.
            // TODO: `!bot config` belongs to the config handler.
            Command::ConfigList { .. }
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
