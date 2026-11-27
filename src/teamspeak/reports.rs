//! The bot's channel and the end of connecting, reported to the bot without ever waiting
//! for it. A report the bot has no room for stays due and goes out on a later tick: a lost
//! channel would leave the bot thinking it is alone, a lost connect skips `on_connect`.

use tokio::sync::mpsc::{Sender, error::TrySendError};
use tracing::{debug, warn};

use super::{CurrentChannel, TeamspeakEvent};

#[derive(Debug, Default)]
pub struct Reports {
    /// The channel the bot last received.
    sent: Option<CurrentChannel>,
    /// A channel the bot has not received yet. Only the latest one matters.
    channel: Option<CurrentChannel>,
    /// `Connected` the bot has not received yet.
    connected: bool,
}

impl Reports {
    /// The channel as it is now. Sent when it differs from the one the bot has.
    pub fn channel(&mut self, events: &Sender<TeamspeakEvent>, current: CurrentChannel) {
        if self.sent == Some(current) {
            self.channel = None;
            return;
        }
        let first = self.channel.is_none();
        self.channel = Some(current);
        self.send_channel(events, first);
    }

    /// The connection is ready, for `on_connect`.
    pub fn connected(&mut self, events: &Sender<TeamspeakEvent>) {
        let first = !self.connected;
        self.connected = true;
        self.send_connected(events, first);
    }

    /// Sends what is still due, oldest kind first. Cheap when nothing is.
    pub fn retry(&mut self, events: &Sender<TeamspeakEvent>) {
        if self.channel.is_some() {
            self.send_channel(events, false);
        }
        if self.connected {
            self.send_connected(events, false);
        }
    }

    /// The connection is gone: after the reconnect everything is sent again, also a
    /// channel that did not change, and nothing from the old connection is due.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn send_channel(&mut self, events: &Sender<TeamspeakEvent>, first: bool) {
        let Some(current) = self.channel else {
            return;
        };
        if self.send(
            events,
            TeamspeakEvent::Channel(current),
            first,
            "a change in its channel",
        ) {
            self.sent = Some(current);
            self.channel = None;
        }
    }

    fn send_connected(&mut self, events: &Sender<TeamspeakEvent>, first: bool) {
        if self.send(events, TeamspeakEvent::Connected, first, "on_connect") {
            self.connected = false;
        }
    }

    /// `true` when the report is done with: sent, or the bot is gone (shutdown).
    /// The first refusal is a warning, the retries every tick only debug lines.
    fn send(
        &self,
        events: &Sender<TeamspeakEvent>,
        event: TeamspeakEvent,
        first: bool,
        what: &str,
    ) -> bool {
        match events.try_send(event) {
            Ok(()) | Err(TrySendError::Closed(_)) => true,
            Err(TrySendError::Full(_)) if first => {
                warn!("the bot is not keeping up, {what} waits");
                false
            }
            Err(TrySendError::Full(_)) => {
                debug!("the bot is still not keeping up, {what} waits");
                false
            }
        }
    }
}
