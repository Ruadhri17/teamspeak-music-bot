//! Bot: the actor that decides. Takes events from the TeamSpeak client, runs commands on
//! the player and sends requests back to the client.
//!
//! One task for all of it, so a command, the end of a track and an event never race:
//! each runs to the end before the next one starts.
//!
//! - `mod.rs`: the actor loop, the outbox to the client and what the player reports
//! - `dispatch`: parsing a chat message, checking permissions, running the command, replies
//! - `lookups`: `!song add`, resolving tracks in the background, queueing them in order

mod dispatch;
mod lookups;

use std::{collections::VecDeque, num::NonZeroUsize, sync::Arc};

use crate::{
    chat,
    config::{Config, SourcesConfig},
    player::{Ended, Launcher, Notice, Player},
    sources::Found,
    teamspeak::{AudioControl, ReplyTo, TeamspeakEvent, TeamspeakRequest},
};
use lookups::Pending;
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};

pub struct Bot {
    player: Player,
    /// Lookups started by `!song add`, matched to `pending` by task ID.
    lookups: JoinSet<Result<Found, String>>,
    /// The lookups in the order of the commands, a finished one waits for those before it.
    pending: VecDeque<Pending>,
    /// From the client: chat messages with commands.
    teamspeak_events: mpsc::Receiver<TeamspeakEvent>,
    /// To the client: chat messages, moving the bot and the like.
    teamspeak_requests: mpsc::Sender<TeamspeakRequest>,
    /// Requests collected while one event is handled, sent in order after it.
    outbox: Vec<TeamspeakRequest>,
    /// For the lookups of `!song add`. One copy shared with the player, so both always
    /// see the same settings.
    // TODO: replaced by the live config once `!bot config set` exists.
    sources: Arc<SourcesConfig>,
    /// Entries per page in `!queue show`.
    page_size: NonZeroUsize,
}

impl Bot {
    pub fn new(
        teamspeak_events: mpsc::Receiver<TeamspeakEvent>,
        teamspeak_requests: mpsc::Sender<TeamspeakRequest>,
        control: watch::Sender<AudioControl>,
        launcher: Box<dyn Launcher>,
        config: &Config,
    ) -> Self {
        let sources = Arc::new(config.sources.clone());
        Self {
            player: Player::new(control, launcher, Arc::clone(&sources), config),
            lookups: JoinSet::new(),
            pending: VecDeque::new(),
            teamspeak_events,
            teamspeak_requests,
            outbox: Vec::new(),
            sources,
            page_size: config.queue.show_page_size,
        }
    }

    /// Runs until the client is gone.
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                event = self.teamspeak_events.recv() => match event {
                    Some(event) => self.handle_event(event),
                    None => break,
                },
                ended = self.player.ended() => self.finished(ended),
                Some(joined) = self.lookups.join_next_with_id() => self.lookup_done(joined),
            }
            self.send_outbox().await;
        }
    }

    /// Sends the outbox to the client, in order.
    async fn send_outbox(&mut self) {
        for request in std::mem::take(&mut self.outbox) {
            // `Err` means the client has stopped, the request is dropped.
            let _ = self.teamspeak_requests.send(request).await;
        }
    }

    /// Queues a chat message for `send_outbox`.
    fn say(&mut self, to: ReplyTo, text: String) {
        self.outbox.push(TeamspeakRequest::SendMessage { to, text });
    }

    fn handle_event(&mut self, event: TeamspeakEvent) {
        let TeamspeakEvent::Message {
            reply_to,
            author,
            text,
        } = event;
        if let Some(text) = self.command(&text, reply_to, &author) {
            self.say(reply_to, text);
        }
    }

    /// A track ended, the player moves on.
    fn finished(&mut self, ended: Ended) {
        self.player.finished(ended);
        self.report();
    }

    /// Handles what the player reported. Called right after every player call that may
    /// start or stop something, so the messages keep their order with the replies.
    fn report(&mut self) {
        for notice in self.player.take_notices() {
            match notice {
                Notice::CouldNotPlay { title, reason } => {
                    let text = format!("could not play {}: {reason}", chat::escape(&title));
                    self.say(ReplyTo::Channel, text);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
