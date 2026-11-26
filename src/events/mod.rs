//! The bot's reactions to its channel, driven by `[events]` (`on_idle`, `on_alone`, `on_party`).
//!
//! A state machine without I/O: it gets facts (the bot's channel, whether music plays, the
//! time) and returns `Action`s for the bot. Deadlines are `Instant`s, the bot sleeps
//! until `next_deadline` and calls `tick`. Tests pass the time directly.

use tokio::time::Instant;

use crate::{
    config::{Event, EventsConfig},
    teamspeak::CurrentChannel,
};

/// What fired an event. Each has its own actions in the config, the conditions differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// Nothing played for `on_idle.delay`, away from home.
    Idle,
    /// Nobody else in the channel for `on_alone.delay`, away from home.
    Alone,
    /// Someone is in the channel and nothing plays, after `on_party.delay`.
    Party,
}

impl Trigger {
    pub const ALL: [Trigger; 3] = [Trigger::Idle, Trigger::Alone, Trigger::Party];

    /// The section in the config, for the log.
    pub fn name(self) -> &'static str {
        match self {
            Trigger::Idle => "on_idle",
            Trigger::Alone => "on_alone",
            Trigger::Party => "on_party",
        }
    }

    /// `on_idle` and `on_alone` usually say goodbye, new music should end them.
    pub fn is_leaving(self) -> bool {
        matches!(self, Trigger::Idle | Trigger::Alone)
    }
}

/// What the bot does in reaction to the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Run the actions of this event from the config.
    Run(Trigger),
    /// Alone at home: stop the music and clear the queue, nobody listens. A rule of the
    /// bot, not configurable.
    StopAtHome,
    /// The bot is in another channel. Stops the sound of an event and cancels its
    /// remaining actions. Home clears the queue.
    Moved { home: bool },
}

pub struct Events {
    config: EventsConfig,
    /// `None` until the client reports the channel after connecting.
    channel: Option<CurrentChannel>,
    /// Music plays (not the sound of an event).
    playing: bool,
    /// Since when nothing plays, `None` while music plays or after `on_idle` fired.
    idle_since: Option<Instant>,
    /// Since when nobody else is in the channel, `None` while someone is or after `on_alone` fired.
    alone_since: Option<Instant>,
    /// When `on_party` is due.
    party_at: Option<Instant>,
    /// `on_idle` or `on_alone` runs, so neither fires again until its actions are done,
    /// music plays or the bot moves.
    departing: bool,
}

impl Events {
    /// The bot starts without music, so the idle time counts from `now`.
    pub fn new(config: &EventsConfig, now: Instant) -> Self {
        Self {
            config: config.clone(),
            channel: None,
            playing: false,
            idle_since: Some(now),
            alone_since: None,
            party_at: None,
            departing: false,
        }
    }

    /// The configured actions of an event.
    pub fn event(&self, trigger: Trigger) -> &Event {
        match trigger {
            Trigger::Idle => &self.config.on_idle,
            Trigger::Alone => &self.config.on_alone,
            Trigger::Party => &self.config.on_party,
        }
    }

    /// The client reported the channel: the bot moved, or someone came or left.
    pub fn channel(&mut self, new: CurrentChannel, now: Instant) -> Vec<Action> {
        let old = self.channel.replace(new);
        let mut actions = Vec::new();

        let moved = old.is_some_and(|old| old.id != new.id);
        if moved {
            actions.push(Action::Moved { home: new.at_home });
            self.departing = false;
            // A new channel is a fresh start, people get the full idle time.
            if !self.playing {
                self.idle_since = Some(now);
            }
        }

        let was_alone = old.is_none_or(|old| moved || old.others == 0);
        if new.others == 0 {
            if moved || old.is_none_or(|old| old.others > 0) {
                self.alone_since = Some(now);
            }
            self.party_at = None;
        } else {
            self.alone_since = None;
            if was_alone && !new.at_home && old.is_some() {
                self.party_at = Some(now + self.config.on_party.delay);
            }
        }

        actions.extend(self.tick(now));
        actions
    }

    /// The bot reports whether music plays, after every change it handled.
    pub fn playing(&mut self, playing: bool, now: Instant) {
        if playing == self.playing {
            return;
        }
        self.playing = playing;
        if playing {
            self.idle_since = None;
            self.departing = false;
            // Music started for nobody, at home that is stopped after `on_alone.delay`.
            if self.channel.is_some_and(|channel| channel.others == 0) {
                self.alone_since = Some(now);
            }
        } else {
            self.idle_since = Some(now);
        }
    }

    /// The bot finished the actions of an event, including the ones after its sound.
    ///
    /// Ends the block of a leaving event (`on_idle`, `on_alone`). Without a `move_to` the bot
    /// stays: the event is over, `on_party` and the other timer work again, and the timer that
    /// fired waits for music or people to change before it can fire again. With a `move_to`
    /// the arrival normally comes right after this and starts everything over. If it does not
    /// (the server refused the move, the channel does not exist), both timers start again,
    /// so the move is retried after their delay instead of the bot staying away for good.
    pub fn actions_done(&mut self, trigger: Trigger, now: Instant) {
        if !trigger.is_leaving() || !self.departing {
            return;
        }
        self.departing = false;
        if self.event(trigger).move_to.is_none() {
            return;
        }
        if !self.playing {
            self.idle_since = Some(now);
        }
        if self.channel.is_some_and(|channel| channel.others == 0) {
            self.alone_since = Some(now);
        }
    }

    /// The earliest moment `tick` has something to do.
    pub fn next_deadline(&self) -> Option<Instant> {
        [self.idle_due(), self.alone_due(), self.party_at]
            .into_iter()
            .flatten()
            .min()
    }

    /// The actions whose time has come.
    pub fn tick(&mut self, now: Instant) -> Vec<Action> {
        let Some(channel) = self.channel else {
            return Vec::new();
        };
        let mut actions = Vec::new();

        if self.party_at.is_some_and(|at| at <= now) {
            self.party_at = None;
            if !self.playing && !self.departing {
                actions.push(Action::Run(Trigger::Party));
            }
        }
        if self.alone_due().is_some_and(|at| at <= now) {
            self.alone_since = None;
            if channel.at_home {
                actions.push(Action::StopAtHome);
            } else {
                self.departing = true;
                actions.push(Action::Run(Trigger::Alone));
            }
        }
        if self.idle_due().is_some_and(|at| at <= now) {
            self.idle_since = None;
            self.departing = true;
            actions.push(Action::Run(Trigger::Idle));
        }
        actions
    }

    /// `on_idle` only fires away from home, at home the bot is idle by design.
    fn idle_due(&self) -> Option<Instant> {
        let channel = self.channel?;
        if channel.at_home || self.departing || self.playing {
            return None;
        }
        Some(self.idle_since? + self.config.on_idle.delay)
    }

    /// At home `on_alone` only matters while music plays.
    fn alone_due(&self) -> Option<Instant> {
        let channel = self.channel?;
        if self.departing || (channel.at_home && !self.playing) {
            return None;
        }
        Some(self.alone_since? + self.config.on_alone.delay)
    }
}

#[cfg(test)]
mod tests;
