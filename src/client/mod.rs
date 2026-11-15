//! Translates server events into `ClientEvent` for the rest of the bot and executes
//! `ClientRequest`s on the connection.
//!
//! - `connect`: building the connection from the config
//! - `event`: `ClientEvent` and translation from tsclientlib events
//! - `request`: `ClientRequest` and executing it on the connection
//! - `error`: `ClientError` for the whole module

mod connect;
mod error;
mod event;
mod request;

pub use error::{ClientError, ClientResult};
pub use event::{ClientEvent, ReplyTo};
pub use request::ClientRequest;

use futures::{StreamExt, future};
use tokio::{
    sync::{mpsc, watch},
    time::MissedTickBehavior,
};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use tsclientlib::{Connection, DisconnectOptions, StreamItem};
use tsproto_packets::packets::{AudioData, CodecType, OutAudio};

use crate::{audio::FRAME_DURATION, config::TeamspeakConfig};

pub struct Client {
    connection: Connection,
    /// To the rest of the bot.
    events: mpsc::Sender<ClientEvent>,
    /// From the rest of the bot.
    requests: mpsc::Receiver<ClientRequest>,
    /// Opus packets from the audio thread, sent one per tick.
    frames: mpsc::Receiver<Vec<u8>>,
    /// While `true`, frames stay in the channel, so the audio thread waits on the full buffer
    /// and resumes exactly where it stopped.
    paused: watch::Receiver<bool>,
    shutdown: CancellationToken,
}

impl Client {
    pub fn new(
        config: &TeamspeakConfig,
        events: mpsc::Sender<ClientEvent>,
        requests: mpsc::Receiver<ClientRequest>,
        frames: mpsc::Receiver<Vec<u8>>,
        paused: watch::Receiver<bool>,
        shutdown: CancellationToken,
    ) -> ClientResult<Self> {
        Ok(Self {
            connection: connect::connect(config)?,
            events,
            requests,
            frames,
            paused,
            shutdown,
        })
    }

    /// Runs until `shutdown` is cancelled or the connection ends.
    ///
    /// The loop must keep polling `events()`: tsclientlib sends outgoing commands only while
    /// events are polled, so a branch that blocks for long also delays the bot's messages.
    pub async fn run(mut self) -> ClientResult<()> {
        let mut ticker = tokio::time::interval(FRAME_DURATION);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                // The `async` block keeps the temporary stream alive while `next()` waits.
                item = async { self.connection.events().next().await } => match item {
                    Some(Ok(item)) => self.handle_item(item).await,
                    Some(Err(source)) => return Err(ClientError::Connection(source.into())),
                    // TODO: decide what a connection closed by the server means (error, reconnect).
                    None => return Ok(()),
                },
                // A failed request (e.g. while tsclientlib reconnects) is not worth stopping the bot,
                // a broken connection is reported by the `events()` branch instead.
                Some(request) = self.requests.recv() => {
                    if let Err(err) = request::execute(&mut self.connection, request) {
                        warn!("{:#}", anyhow::Error::from(err));
                    }
                }
                _ = ticker.tick() => self.send_frame(),
                _ = self.shutdown.cancelled() => break,
            }
        }

        self.disconnect().await
    }

    async fn handle_item(&mut self, item: StreamItem) {
        // TODO: `DisconnectedTemporarily` (tsclientlib reconnects on its own), log it at warn.
        let StreamItem::BookEvents(events) = item else {
            return;
        };

        let Ok(state) = self.connection.get_state() else {
            // Book events only come after connecting, so this should not happen.
            warn!("received events before the connection state exists, skipped");
            return;
        };
        let bot_id = state.own_client;

        for event in events {
            if let Some(event) = event::translate(&event, bot_id) {
                // `send` waits while the channel is full, which pauses this loop.
                // `Err` means the receiver is gone, so the bot is shutting down anyway.
                let _ = self.events.send(event).await;
            }
        }
    }

    /// Called every `FRAME_DURATION`. The clock sets the pace, the channel is only storage.
    fn send_frame(&mut self) {
        // `borrow()` holds a read lock, the deref copies the bool and drops it right away.
        if *self.paused.borrow() {
            return;
        }
        // Empty is the normal state when nothing plays.
        let Ok(data) = self.frames.try_recv() else {
            return;
        };

        let packet = OutAudio::new(&AudioData::C2S {
            id: 0,
            codec: CodecType::OpusMusic,
            data: &data,
        });

        if let Err(err) = self.connection.send_audio(packet) {
            warn!("audio could not be sent: {err}");
        }
    }

    /// Tells the server the bot is leaving, instead of disappearing with a timeout.
    async fn disconnect(mut self) -> ClientResult<()> {
        // TODO: `events.on_disconnect.message` as the reason shown to users.
        self.connection
            .disconnect(DisconnectOptions::new())
            .map_err(|source| ClientError::Request(source.into()))?;
        // Keep polling until the server confirms, the disconnect only goes out while polling.
        self.connection
            .events()
            .for_each(|_| future::ready(()))
            .await;
        Ok(())
    }
}
