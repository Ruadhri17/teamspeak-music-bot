//! What the client knows about its connection, handed to the bot as values in `watch`
//! channels. These are states, not events: only the latest channel matters, connecting is
//! counted, and streams are heard in order, so the last one heard says it all. The bot
//! reads the newest value when it has time, nothing is lost and the client never waits for
//! it, which keeps the sound going.

use tokio::sync::watch;

use super::CurrentChannel;

/// The client's side.
#[derive(Debug)]
pub struct StateTx {
    channel: watch::Sender<Option<CurrentChannel>>,
    connects: watch::Sender<u64>,
    heard: watch::Sender<Option<u64>>,
}

/// The bot's side.
#[derive(Debug)]
pub struct StateRx {
    /// The bot's channel, `None` until it is known and while reconnecting.
    pub channel: watch::Receiver<Option<CurrentChannel>>,
    /// How many times the client finished connecting, each one is an `on_connect`.
    pub connects: watch::Receiver<u64>,
    /// Every stream up to this one was heard to its end.
    pub heard: watch::Receiver<Option<u64>>,
}

pub fn state() -> (StateTx, StateRx) {
    let (channel_tx, channel) = watch::channel(None);
    let (connects_tx, connects) = watch::channel(0);
    let (heard_tx, heard) = watch::channel(None);
    (
        StateTx {
            channel: channel_tx,
            connects: connects_tx,
            heard: heard_tx,
        },
        StateRx {
            channel,
            connects,
            heard,
        },
    )
}

impl StateTx {
    /// The bot hears of it only when it differs from what it has. `None` while
    /// reconnecting, so the same channel after the reconnect counts as a change.
    pub fn channel(&self, current: Option<CurrentChannel>) {
        self.channel.send_if_modified(|old| {
            let changed = *old != current;
            *old = current;
            changed
        });
    }

    pub fn connected(&self) {
        self.connects.send_modify(|count| *count += 1);
    }

    /// Stream IDs only go up, an older one changes nothing.
    pub fn heard(&self, stream: u64) {
        self.heard.send_if_modified(|heard| {
            let newer = heard.is_none_or(|heard| stream > heard);
            if newer {
                *heard = Some(stream);
            }
            newer
        });
    }
}
