use tsclientlib::{ClientId, Invoker, MessageTarget, events::Event};

use crate::permissions::Role;

/// Server event in our own types, so the rest of the bot does not depend on tsclientlib.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    /// Chat message in the bot's channel, a private message or a poke to the bot.
    Message {
        reply_to: ReplyTo,
        sender: Sender,
        text: String,
    },
}

/// Who sent a message, as the client sees them when it arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    /// The client ID on the server, valid while they stay connected.
    pub id: u16,
    pub name: String,
    pub role: Role,
    /// `false` also when the bot cannot see the sender's channel.
    pub in_bot_channel: bool,
}

/// Where an answer goes: the same place the message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyTo {
    Channel,
    /// Private chat with the client of this ID.
    Client(u16),
}

impl From<ReplyTo> for MessageTarget {
    fn from(reply_to: ReplyTo) -> Self {
        match reply_to {
            ReplyTo::Channel => MessageTarget::Channel,
            ReplyTo::Client(id) => MessageTarget::Client(ClientId(id)),
        }
    }
}

/// Returns `None` for events the bot does not react to, including its own messages.
/// `sender` looks up the role and channel of the message's author.
pub fn translate(
    event: &Event,
    bot_id: ClientId,
    sender: impl FnOnce(&Invoker) -> Sender,
) -> Option<ClientEvent> {
    // TODO: later: clients joining and leaving the bot's channel, for `on_alone` and `on_party`.
    match event {
        Event::Message {
            target,
            invoker,
            message,
        } if invoker.id != bot_id => {
            let reply_to = match target {
                MessageTarget::Channel => ReplyTo::Channel,
                // Replies to a poke go to the private chat, pokes are limited to 100 characters.
                MessageTarget::Client(_) | MessageTarget::Poke(_) => ReplyTo::Client(invoker.id.0),
                MessageTarget::Server => return None,
            };
            Some(ClientEvent::Message {
                reply_to,
                sender: sender(invoker),
                text: message.clone(),
            })
        }
        _ => None,
    }
}
