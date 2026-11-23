//! The bot's home channel and the decision for `!bot summon`.
//!
//! Works on plain channel IDs and names, so it is tested without a server.

use crate::config::ChannelRef;

/// What the client needs to know about one channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelInfo<'a> {
    pub id: u64,
    /// 0 for a top-level channel.
    pub parent: u64,
    pub name: &'a str,
    pub is_default: bool,
}

/// The home channel from the config, or the server's default channel when it is not set.
/// `None` when the configured channel does not exist.
pub fn resolve(configured: Option<&ChannelRef>, channels: &[ChannelInfo]) -> Option<u64> {
    match configured {
        Some(ChannelRef::Id(id)) => channels.iter().any(|c| c.id == *id).then_some(*id),
        Some(ChannelRef::Name(path)) => find_path(path, channels),
        None => channels.iter().find(|c| c.is_default).map(|c| c.id),
    }
}

/// `Parent/Channel`, each part matched exactly from the top level down.
fn find_path(path: &str, channels: &[ChannelInfo]) -> Option<u64> {
    path.split('/').try_fold(0, |parent, name| {
        channels
            .iter()
            .find(|c| c.parent == parent && c.name == name)
            .map(|c| c.id)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Summon {
    /// The sender is already in the bot's channel.
    Stay,
    /// The bot is at home and goes to the sender's channel.
    Move(u64),
    /// The bot is in someone else's channel.
    Busy,
}

// TODO: admins summon the bot even while it is busy.
pub fn summon(bot: u64, home: u64, sender: u64) -> Summon {
    if sender == bot {
        Summon::Stay
    } else if bot == home {
        Summon::Move(sender)
    } else {
        Summon::Busy
    }
}
