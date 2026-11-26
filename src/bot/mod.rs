//! Bot: the actor that decides. Takes events from the TeamSpeak client, runs commands on
//! the player, reacts to the channel and sends requests back to the client.
//!
//! One task for all of it, so a command, the end of a track and an event never race:
//! each runs to the end before the next one starts.
//!
//! - `mod.rs`: the actor loop, the outbox to the client and what the player reports
//! - `dispatch`: parsing a chat message, checking permissions, running the command, replies
//! - `lookups`: `!song add`, resolving tracks in the background, queueing them in order
//! - `actions`: running the actions of events, their sounds

mod actions;
mod dispatch;
mod lookups;

use std::{
    collections::{HashMap, VecDeque},
    num::NonZeroUsize,
    sync::Arc,
};

use crate::{
    chat,
    config::{Config, SourcesConfig},
    events::{Events, Trigger},
    player::{Ended, Launcher, Notice, Player},
    sources::{Found, TrackRef},
    teamspeak::{AudioControl, ReplyTo, TeamspeakEvent, TeamspeakRequest},
};
use actions::SoundLookup;
use lookups::Pending;
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
    time::{Instant, sleep_until},
};

pub struct Bot {
    player: Player,
    /// Reactions to the bot's channel from `[events]`. Fed with the channel and whether
    /// music plays, returns the actions to run.
    events: Events,
    /// The sounds of events that are ready to play: files at once, links once found.
    sounds: HashMap<Trigger, TrackRef>,
    /// Sounds given as links, looked up once after the start.
    sound_lookups: JoinSet<SoundLookup>,
    /// The event whose sound plays now, its remaining actions wait for the end.
    waiting: Option<Trigger>,
    /// Lookups started by `!song add`, matched to `pending` by task ID.
    lookups: JoinSet<Result<Found, String>>,
    /// The lookups in the order of the commands, a finished one waits for those before it.
    pending: VecDeque<Pending>,
    /// From the client: chat messages with commands and changes in the bot's channel.
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
        let mut bot = Self {
            player: Player::new(control, launcher, Arc::clone(&sources), config),
            events: Events::new(&config.events, Instant::now()),
            sounds: HashMap::new(),
            sound_lookups: JoinSet::new(),
            waiting: None,
            lookups: JoinSet::new(),
            pending: VecDeque::new(),
            teamspeak_events,
            teamspeak_requests,
            outbox: Vec::new(),
            sources,
            page_size: config.queue.show_page_size,
        };
        bot.prepare_sounds();
        bot
    }

    /// Runs until the client is gone.
    pub async fn run(mut self) {
        loop {
            // Every turn, the deadlines change with each event. Taken before `select!`,
            // where the other branches already borrow fields of `self`.
            let deadline = self.events.next_deadline();
            tokio::select! {
                event = self.teamspeak_events.recv() => match event {
                    Some(event) => self.handle_event(event),
                    None => break,
                },
                ended = self.player.ended() => self.finished(ended),
                Some(joined) = self.lookups.join_next_with_id() => self.lookup_done(joined),
                Some(joined) = self.sound_lookups.join_next() => self.sound_found(joined),
                // A disabled branch still builds its future, so `unwrap` would panic without
                // a deadline. The placeholder is never awaited.
                _ = sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                    let actions = self.events.tick(Instant::now());
                    self.run_actions(actions);
                }
            }
            // After every branch, not in each command: music starts and stops through
            // commands, finished tracks and event actions alike.
            self.events
                .playing(self.player.is_playing(), Instant::now());
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
        match event {
            TeamspeakEvent::Message {
                reply_to,
                author,
                text,
            } => {
                if let Some(text) = self.command(&text, reply_to, &author) {
                    self.say(reply_to, text);
                }
            }
            TeamspeakEvent::Channel(current) => {
                let actions = self.events.channel(current, Instant::now());
                self.run_actions(actions);
            }
        }
    }

    /// A track ended, the player moves on.
    fn finished(&mut self, ended: Ended) {
        self.player.finished(ended);
        self.report();
    }

    /// Handles what the player reported. Called right after every player call that may
    /// start or stop something, so the messages keep their order with the replies.
    /// Repeats until nothing is left: the end of a welcome starts the queue, which may
    /// report again.
    fn report(&mut self) {
        loop {
            let notices = self.player.take_notices();
            if notices.is_empty() {
                return;
            }
            for notice in notices {
                match notice {
                    Notice::CouldNotPlay { title, reason } => {
                        let text = format!("could not play {}: {reason}", chat::escape(&title));
                        self.say(ReplyTo::Channel, text);
                    }
                    Notice::SoundEnded => self.sound_ended(),
                    Notice::SoundInterrupted => self.sound_interrupted(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
