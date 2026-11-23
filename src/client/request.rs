use tracing::{debug, warn};
use tsclientlib::{ChannelId, ClientId, Connection, MessageHandle, OutCommandExt};

use super::{
    ClientError, ClientResult, ReplyTo,
    home::{self, Summon},
};
use crate::chat;

/// What the rest of the bot asks the client to do on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientRequest {
    SendMessage {
        to: ReplyTo,
        text: String,
    },
    /// `!bot summon`: move to the sender's channel if the bot is at home.
    Summon {
        sender_id: u16,
        reply_to: ReplyTo,
    },
    // TODO: `SetNickname(text)` and `SetDescription(text)` for `!bot config set`.
}

/// A command sent to the server, its result comes later as `StreamItem::MessageResult`.
pub type Sent = (MessageHandle, &'static str);

/// `home` is `None` until the client knows its home channel after connecting.
/// `Err` only when the command cannot leave the client, e.g. while tsclientlib reconnects.
/// A rejection by the server (missing permissions, flood protection) arrives later for each
/// returned `Sent`, the label names the command in the log.
pub fn execute(
    connection: &mut Connection,
    home: Option<ChannelId>,
    request: ClientRequest,
) -> ClientResult<Option<Sent>> {
    match request {
        ClientRequest::SendMessage { to, text } => send_message(connection, to, &text).map(Some),
        ClientRequest::Summon {
            sender_id,
            reply_to,
        } => summon(connection, home, sender_id, reply_to),
    }
}

fn send_message(connection: &mut Connection, to: ReplyTo, text: &str) -> ClientResult<Sent> {
    let handle = state(connection)?
        .send_message(to.into(), text)
        .send_with_result(connection)
        .map_err(request_error)?;
    Ok((handle, "a chat message"))
}

fn summon(
    connection: &mut Connection,
    home: Option<ChannelId>,
    sender_id: u16,
    reply_to: ReplyTo,
) -> ClientResult<Option<Sent>> {
    let Some(home) = home else {
        warn!("!bot summon before the home channel is known, ignored");
        return Ok(None);
    };
    let state = state(connection)?;
    let (Some(bot), Some(sender)) = (
        state.clients.get(&state.own_client),
        state.clients.get(&ClientId(sender_id)),
    ) else {
        debug!(
            "!bot summon from a client the bot cannot see (left, or in an unsubscribed channel), ignored"
        );
        return Ok(None);
    };

    let decision = home::summon(bot.channel.0, home.0, sender.channel.0);
    debug!(
        "!bot summon: bot in {}, home {}, sender in {}: {decision:?}",
        bot.channel.0, home.0, sender.channel.0
    );
    match decision {
        Summon::Stay => Ok(None),
        Summon::Move(channel) => {
            let handle = bot
                .client_move(ChannelId(channel))
                .send_with_result(connection)
                .map_err(request_error)?;
            Ok(Some((handle, "moving to the sender's channel")))
        }
        Summon::Busy => {
            let name = state
                .channels
                .get(&bot.channel)
                .map_or("another channel", |channel| channel.name.as_str());
            let text = format!("I am busy in {}", chat::escape(name));
            send_message(connection, reply_to, &text).map(Some)
        }
    }
}

fn state(connection: &Connection) -> ClientResult<&tsclientlib::data::Connection> {
    connection.get_state().map_err(request_error)
}

fn request_error(source: tsclientlib::Error) -> ClientError {
    ClientError::Request(Box::new(source))
}
