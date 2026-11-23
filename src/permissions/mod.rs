//! Permissions: admin -> blocked -> user, by server groups (name or ID).
//!
//! The "same channel as the bot" rule and summoning from the home channel.
//! Dragging the bot to another channel is left to the server permissions.
//! Pure functions: the client supplies the sender's groups and channel.

use thiserror::Error;

use crate::{
    commands::Command,
    config::{GroupRef, PermissionsConfig},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Controls the bot from any channel and summons it while it is busy.
    Admin,
    /// May not control or summon the bot.
    Blocked,
    /// Controls the bot from its channel and summons it while it is at home.
    User,
}

/// One server group of the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group<'a> {
    pub id: u64,
    /// `None` when the bot cannot see the server's group list, then only IDs match.
    pub name: Option<&'a str>,
}

/// Admin takes precedence, so a blocked group cannot lock admins out by mistake.
pub fn role(config: &PermissionsConfig, groups: &[Group]) -> Role {
    if any_matches(&config.admin_groups, groups) {
        Role::Admin
    } else if any_matches(&config.blocked_groups, groups) {
        Role::Blocked
    } else {
        Role::User
    }
}

fn any_matches(configured: &[GroupRef], groups: &[Group]) -> bool {
    configured
        .iter()
        .any(|wanted| groups.iter().any(|group| matches(wanted, group)))
}

fn matches(wanted: &GroupRef, group: &Group) -> bool {
    match wanted {
        GroupRef::Id(id) => group.id == *id,
        GroupRef::Name(name) => group.name.is_some_and(|actual| same_name(actual, name)),
    }
}

/// Group names compare without case, also outside ASCII (`Właściciel`).
pub fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Why a command was refused. The text goes to the chat, without configuration details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Denied {
    #[error("you are not allowed to use this bot")]
    Blocked,
    #[error("join my channel to control me, or summon me with !bot summon")]
    OtherChannel,
    #[error("only admins can use this command")]
    AdminOnly,
}

/// A command is allowed for an admin, or for a user in the bot's channel.
/// `!help`, `!bot ping` and `!bot summon` work from anywhere, summon decides on its own
/// whether the bot comes.
pub fn check(command: &Command, role: Role, in_bot_channel: bool) -> Result<(), Denied> {
    match role {
        Role::Admin => Ok(()),
        Role::Blocked => Err(Denied::Blocked),
        Role::User => match command {
            Command::ConfigList { .. }
            | Command::ConfigGet { .. }
            | Command::ConfigSet { .. }
            | Command::ConfigReload => Err(Denied::AdminOnly),
            Command::Help(_) | Command::Ping | Command::Summon => Ok(()),
            _ if in_bot_channel => Ok(()),
            _ => Err(Denied::OtherChannel),
        },
    }
}

#[cfg(test)]
mod tests;
