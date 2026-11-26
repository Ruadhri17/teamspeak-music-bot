use tsclientlib::{ClientId, Invoker, MessageTarget, events::Event};

use crate::permissions::Role;

/// Server event in our own types, so the rest of the bot does not depend on tsclientlib.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamspeakEvent {
    /// Chat message in the bot's channel, a private message or a poke to the bot.
    Message {
        reply_to: ReplyTo,
        author: Author,
        text: String,
    },
    /// The bot moved, or someone came to or left its channel.
    Channel(CurrentChannel),
    /// The bot connected or reconnected and knows its home channel. Comes after the first
    /// `Channel` of the connection.
    Connected,
}

/// Who sent a message, as the client sees them when it arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    /// The client ID on the server, valid while they stay connected.
    pub id: u16,
    pub name: String,
    pub role: Role,
    /// `false` also when the bot cannot see the author's channel.
    pub in_bot_channel: bool,
}

/// Where the bot is right now, sent again whenever it moves or someone comes or leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentChannel {
    pub id: u64,
    pub at_home: bool,
    /// Other people in the channel, without the bot and server query clients.
    pub others: usize,
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
/// `author` looks up the role and channel of the one who wrote the message.
pub fn translate(
    event: &Event,
    bot_id: ClientId,
    author: impl FnOnce(&Invoker) -> Author,
) -> Option<TeamspeakEvent> {
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
            Some(TeamspeakEvent::Message {
                reply_to,
                author: author(invoker),
                text: message.clone(),
            })
        }
        _ => None,
    }
}
