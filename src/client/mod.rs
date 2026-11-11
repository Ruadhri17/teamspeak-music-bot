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
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use tsclientlib::{Connection, DisconnectOptions, StreamItem};

use crate::config::TeamspeakConfig;

pub struct Client {
    connection: Connection,
    /// To the rest of the bot.
    events: mpsc::Sender<ClientEvent>,
    /// From the rest of the bot.
    requests: mpsc::Receiver<ClientRequest>,
    shutdown: CancellationToken,
}

impl Client {
    pub fn new(
        config: &TeamspeakConfig,
        events: mpsc::Sender<ClientEvent>,
        requests: mpsc::Receiver<ClientRequest>,
        shutdown: CancellationToken,
    ) -> ClientResult<Self> {
        Ok(Self {
            connection: connect::connect(config)?,
            events,
            requests,
            shutdown,
        })
    }

    /// Runs until `shutdown` is cancelled or the connection ends.
    ///
    /// The loop must keep polling `events()`: tsclientlib sends outgoing commands only while
    /// events are polled, so a branch that blocks for long also delays the bot's messages.
    pub async fn run(mut self) -> ClientResult<()> {
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
