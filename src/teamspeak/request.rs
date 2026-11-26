use tracing::{debug, warn};
use tsclientlib::{
    ChannelId, ClientId, Connection, MessageHandle, OutCommandExt,
    prelude::{M2BClientEditExt, M2BClientUpdateExt},
};

use super::{
    ReplyTo, TeamspeakError, TeamspeakResult, channel_infos,
    home::{self, Summon},
};
use crate::{chat, config::Destination};

/// What the rest of the bot asks the client to do on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamspeakRequest {
    SendMessage {
        to: ReplyTo,
        text: String,
    },
    /// `!bot summon`: move to the sender's channel if the bot is at home, or always for an admin.
    Summon {
        sender_id: u16,
        reply_to: ReplyTo,
        admin: bool,
    },
    /// `move_to` of an event.
    Move(Destination),
    /// From an event, later also from `!bot config set`. Empty clears the description.
    SetDescription(String),
    /// From an event, later also from `!bot config set`.
    SetNickname(String),
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
    request: TeamspeakRequest,
) -> TeamspeakResult<Option<Sent>> {
    match request {
        TeamspeakRequest::SendMessage { to, text } => send_message(connection, to, &text).map(Some),
        TeamspeakRequest::Summon {
            sender_id,
            reply_to,
            admin,
        } => summon(connection, home, sender_id, reply_to, admin),
        TeamspeakRequest::Move(destination) => move_bot(connection, destination, home),
        TeamspeakRequest::SetDescription(text) => set_description(connection, &text),
        TeamspeakRequest::SetNickname(name) => set_nickname(connection, &name),
    }
}

fn send_message(connection: &mut Connection, to: ReplyTo, text: &str) -> TeamspeakResult<Sent> {
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
    admin: bool,
) -> TeamspeakResult<Option<Sent>> {
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

    let decision = home::summon(bot.channel.0, home.0, sender.channel.0, admin);
    debug!(
        "!bot summon: bot in {}, home {}, sender in {}, admin {admin}: {decision:?}",
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

/// Moves the bot to its home or a channel from the config. Does nothing when the bot is
/// already there, the server would refuse it as "already member of channel".
fn move_bot(
    connection: &mut Connection,
    destination: Destination,
    home: Option<ChannelId>,
) -> TeamspeakResult<Option<Sent>> {
    let state = state(connection)?;
    let Some(bot) = state.clients.get(&state.own_client) else {
        debug!("move: the bot is not in the connection state, ignored");
        return Ok(None);
    };

    let target = match destination {
        Destination::Home => match home {
            Some(home) => home,
            None => {
                warn!("move home before the home channel is known, ignored");
                return Ok(None);
            }
        },
        Destination::Channel(channel) => {
            match home::resolve(Some(&channel), &channel_infos(state)) {
                Some(id) => ChannelId(id),
                None => {
                    // From the config, not from users, so it is safe to log.
                    warn!("move_to: channel {channel:?} not found on the server, ignored");
                    return Ok(None);
                }
            }
        }
    };
    if bot.channel == target {
        return Ok(None);
    }

    let handle = bot
        .client_move(target)
        .send_with_result(connection)
        .map_err(request_error)?;
    Ok(Some((handle, "moving the bot")))
}

/// An empty text clears the description. Needs `b_client_modify_own_description`.
fn set_description(connection: &mut Connection, text: &str) -> TeamspeakResult<Option<Sent>> {
    let state = state(connection)?;
    let Some(bot) = state.clients.get(&state.own_client) else {
        debug!("description: the bot is not in the connection state, ignored");
        return Ok(None);
    };
    if bot.description == text {
        return Ok(None);
    }
    let handle = bot
        .edit()
        .set_description(text)
        .send_with_result(connection)
        .map_err(request_error)?;
    Ok(Some((handle, "setting the description")))
}

/// The bot can only rename itself, so this goes through the connection, not a client.
/// tsclientlib keeps the new name for a reconnect. A name in use is refused by the server.
fn set_nickname(connection: &mut Connection, name: &str) -> TeamspeakResult<Option<Sent>> {
    let state = state(connection)?;
    if state
        .clients
        .get(&state.own_client)
        .is_some_and(|bot| bot.name == name)
    {
        return Ok(None);
    }
    let handle = state
        .client_update()
        .set_name(name)
        .send_with_result(connection)
        .map_err(request_error)?;
    Ok(Some((handle, "changing the nickname")))
}

fn state(connection: &Connection) -> TeamspeakResult<&tsclientlib::data::Connection> {
    connection.get_state().map_err(request_error)
}

fn request_error(source: tsclientlib::Error) -> TeamspeakError {
    TeamspeakError::Request(Box::new(source))
}
