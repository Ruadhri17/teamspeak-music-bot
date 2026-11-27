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
    config::{Config, SourcesConfig, Volume},
    events::{Events, Trigger},
    player::{Ended, Launcher, Notice, Player},
    sources::{Found, TrackRef},
    teamspeak::{
        AudioControl, ClientEvent, CurrentChannel, ReplyTo, TeamspeakEvent, TeamspeakRequest,
    },
};
use actions::SoundLookup;
use lookups::Pending;
use tokio::{
    sync::{Semaphore, mpsc, watch},
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
    /// Lets `sources.max_parallel_lookups` lookups run at once, the others wait in order.
    lookup_slots: Arc<Semaphore>,
    /// From the client: chat messages with commands.
    teamspeak_events: mpsc::Receiver<TeamspeakEvent>,
    /// From the client: the bot's channel, connecting and the heard streams, in order.
    client_events: mpsc::UnboundedReceiver<ClientEvent>,
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
    /// `teamspeak.nickname` and `teamspeak.description`, for `{default}` in events.
    default_nickname: String,
    default_description: String,
    /// `set_volume = "{default}"` of events.
    default_volume: Volume,
}

impl Bot {
    pub fn new(
        teamspeak_events: mpsc::Receiver<TeamspeakEvent>,
        client_events: mpsc::UnboundedReceiver<ClientEvent>,
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
            lookup_slots: Arc::new(Semaphore::new(config.sources.max_parallel_lookups.get())),
            teamspeak_events,
            client_events,
            teamspeak_requests,
            outbox: Vec::new(),
            sources,
            page_size: config.queue.show_page_size,
            default_nickname: config.teamspeak.nickname.clone(),
            default_description: config.teamspeak.description.clone(),
            default_volume: config.audio.default_volume,
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
                // `None` means the client is gone, the events branch ends the loop then.
                Some(event) = self.client_events.recv() => self.client_event(event),
                ended = self.player.ended() => self.finished(ended),
                Some(joined) = self.lookups.join_next_with_id() => self.lookup_done(joined),
                Some(joined) = self.sound_lookups.join_next() => self.sound_found(joined),
                // A disabled branch still builds its future, so `unwrap` would panic without
                // a deadline. The placeholder is never awaited.
                _ = sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                    let actions = self.events.tick(self.player.is_playing(), Instant::now());
                    self.run_actions(actions);
                }
            }
            self.end_turn();
            self.send_outbox().await;
        }
    }

    /// After every turn of `run`, whatever happened in it: songs that waited for something
    /// that is over start, and the events learn whether music plays, for their deadlines.
    fn end_turn(&mut self) {
        self.start_waiting_songs();
        self.events
            .playing(self.player.is_playing(), Instant::now());
    }

    /// The one place that starts songs that waited: for the sound of an event, which ended,
    /// or was cut short by a move. Nothing plays (a sound of an event playing is not idle
    /// either), nothing holds a pause, no event waits for its sound, and the queue has
    /// songs. A new welcome in the same turn plays first, the songs wait for it. Only
    /// ending a goodbye starts songs elsewhere, see `end_goodbye`.
    fn start_waiting_songs(&mut self) {
        if self.waiting.is_none()
            && self.player.is_idle()
            && !self.player.is_held()
            && !self.player.queue_is_empty()
        {
            self.player.start_queue();
            self.report();
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
        }
    }

    /// The client's channel, connecting and heard streams, in the order they happened.
    fn client_event(&mut self, event: ClientEvent) {
        match event {
            ClientEvent::Channel(current) => self.channel_changed(current),
            ClientEvent::Connected => self.connected(),
            ClientEvent::Heard(stream) => self.heard(stream),
        }
    }

    /// The bot moved, or someone came to or left its channel.
    fn channel_changed(&mut self, current: CurrentChannel) {
        let actions = self
            .events
            .channel(current, self.player.is_playing(), Instant::now());
        self.run_actions(actions);
    }

    /// The client finished connecting.
    fn connected(&mut self) {
        let actions = self
            .events
            .connected(self.player.is_playing(), Instant::now());
        self.run_actions(actions);
    }

    /// Every stream up to `stream` was heard to its end.
    fn heard(&mut self, stream: u64) {
        self.player.played(stream);
        self.report();
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
                    Notice::SongStarted => {
                        // The song plays already: a timer due now must not see the
                        // silence before it, or `on_idle` would end it at once.
                        let actions = self
                            .events
                            .song_started(self.player.is_playing(), Instant::now());
                        self.run_actions(actions);
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
