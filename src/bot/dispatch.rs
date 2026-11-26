//! A chat message to a command: parsing, permissions, running it on the player and the
//! reply. Success is silent, except for informational commands.

use std::num::NonZeroUsize;

use tracing::debug;

use super::Bot;
use crate::{
    chat,
    commands::Command,
    permissions::{self, Role},
    teamspeak::{Author, ReplyTo, TeamspeakRequest},
};

impl Bot {
    /// Parses a chat message, checks the author's permissions and runs the command.
    /// Returns the chat reply, `None` for text that is not a command.
    pub(super) fn command(
        &mut self,
        text: &str,
        reply_to: ReplyTo,
        author: &Author,
    ) -> Option<String> {
        let command = match Command::parse(text)? {
            Ok(command) => command,
            Err(err) => return Some(err.to_string()),
        };
        if let Err(denied) = permissions::check(&command, author.role, author.in_bot_channel) {
            debug!("command from {} refused: {denied}", author.name);
            return Some(denied.to_string());
        }
        self.execute(command, reply_to, author)
    }

    /// Runs a command and returns the chat reply, if any.
    fn execute(&mut self, command: Command, reply_to: ReplyTo, author: &Author) -> Option<String> {
        match command {
            Command::Add { query, source, .. } => {
                self.add(query, source, reply_to);
                None
            }
            Command::Pause => {
                self.player.pause();
                None
            }
            Command::Resume => {
                self.end_goodbye();
                let result = self.player.resume();
                self.report();
                result.err().map(|err| err.to_string())
            }
            Command::Next => {
                self.player.next();
                self.report();
                None
            }
            Command::Current => Some(self.current()),
            Command::Volume(None) => Some(format!("volume: {}", self.player.volume().get())),
            Command::Volume(Some(volume)) => self
                .player
                .set_volume(volume)
                .err()
                .map(|err| err.to_string()),
            Command::QueueShow { page } => Some(self.queue_show(page)),
            Command::QueueRemove { position } => self
                .player
                .queue_remove(position)
                .err()
                .map(|err| err.to_string()),
            Command::QueueClear => {
                self.queue_clear();
                None
            }
            Command::Ping => Some("pong".to_string()),
            Command::Help(text) => Some(text.to_string()),
            // The client knows the channels, it decides and answers.
            Command::Summon => {
                self.outbox.push(TeamspeakRequest::Summon {
                    sender_id: author.id,
                    reply_to,
                    admin: author.role == Role::Admin,
                });
                None
            }
            // Listed one by one, so a new command does not compile until it is handled.
            // TODO: `!bot config` belongs to the config handler.
            Command::ConfigList { .. }
            | Command::ConfigGet { .. }
            | Command::ConfigSet { .. }
            | Command::ConfigReload => None,
        }
    }

    /// `!song current`. Always replies (informational command).
    // TODO: artist, source and position in time, once sources give that.
    fn current(&self) -> String {
        match self.player.current() {
            None => "nothing is playing".to_string(),
            Some(current) if current.paused => {
                format!("{} (paused)", chat::escape(&current.track.title))
            }
            Some(current) => chat::escape(&current.track.title).into_owned(),
        }
    }

    /// `!queue show [page]`. Always replies (informational command).
    // TODO: TS3 messages are limited to about 1024 characters, long titles need cutting.
    fn queue_show(&self, page: NonZeroUsize) -> String {
        let Some(page) = self.player.queue_page(page, self.page_size) else {
            return "no such page, the queue is shorter".to_string();
        };

        let mut lines = Vec::new();
        if let Some(current) = self.player.current() {
            lines.push(format!("Now: {}", chat::escape(&current.track.title)));
        }
        if page.entries.is_empty() {
            lines.push("the queue is empty".to_string());
        }
        for (position, track) in &page.entries {
            lines.push(format!("{position}. {}", chat::escape(&track.title)));
        }
        if page.count > 1 {
            lines.push(format!("page {}/{}", page.number, page.count));
        }
        lines.join("\n")
    }

    /// `!queue clear`. Empties the queue, stops the current track and cancels the lookups
    /// of `!song add` that have not finished yet.
    pub(super) fn queue_clear(&mut self) {
        self.lookups.abort_all();
        self.pending.clear();
        self.player.clear();
        self.report();
    }
}
