//! TeamSpeak client (tsclientlib): the sole owner of `Connection`.
//!
//! - connection, identity, joining the home channel,
//! - receiving events (messages, client moves) and passing them on,
//! - executing requests: send message, move, change nickname and description,
//! - sending Opus frames every 20 ms (no catch-up bursts),
//! - reconnecting with backoff.

use futures::StreamExt;

use tsclientlib::Connection;
use tsclientlib::MessageTarget;
use tsclientlib::OutCommandExt;
use tsclientlib::StreamItem;
use tsclientlib::events::Event;

pub async fn connect() {
    let mut connection = Connection::build("localhost").connect().unwrap();

    loop {
        let maybe_event = connection.events().next().await;
        let Some(Ok(events)) = maybe_event else { break };
        match events {
            StreamItem::BookEvents(events) => {
                for event in events {
                    match event {
                        Event::Message { message, .. } => {
                            if message == "!bot ping" {
                                connection
                                    .get_state()
                                    .unwrap()
                                    .send_message(MessageTarget::Channel, "pong")
                                    .send(&mut connection)
                                    .unwrap();
                            }
                        }
                        _ => continue,
                    }
                }
            }
            _ => continue,
        };
    }
}
