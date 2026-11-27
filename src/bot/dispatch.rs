//! A chat message to a command: parsing, permissions, running it on the player and the
//! reply. Success is silent, except for informational commands.

use std::num::NonZeroUsize;

use tracing::debug;

use super::Bot;
use crate::{
    chat,
    commands::Command,
    permissions::{self, Role},
    sources::{QueryType, SearchKind, TrackRef},
    teamspeak::{Author, ReplyTo, TeamspeakRequest},
};

/// The longest title in `!queue show`: 15 entries of this length fit in one message.
const QUEUE_TITLE_LEN: usize = 50;

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
            // Links carry their own type, so the flags only matter for a phrase.
            Command::Add {
                query: QueryType::Phrase(_),
                kind: SearchKind::Album | SearchKind::Playlist,
                ..
            } => Some(
                "searching for albums and playlists is not supported yet, add a playlist link instead"
                    .to_string(),
            ),
            Command::Add { query, source, .. } => {
                // Refused at once: after the lookup it would be refused anyway, and a lookup
                // can be a yt-dlp run of several seconds of CPU on a small server.
                if let Some(full) = self.player.queue_full() {
                    return Some(full.to_string());
                }
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
            | Command::ConfigReload => Some("!bot config is not supported yet".to_string()),
        }
    }

    /// `!song current`. Always replies (informational command).
    // TODO: artist, source and position in time, once sources give that.
    fn current(&self) -> String {
        match self.player.current() {
            None => "nothing is playing".to_string(),
            Some(current) if current.paused => {
                format!("{} (paused)", chat::escape(current.track.display_name()))
            }
            Some(current) => chat::escape(current.track.display_name()).into_owned(),
        }
    }

    /// `!queue show [page]`. Always replies (informational command), in one message.
    /// Every line gets an equal share of the message and titles are cut to it, counted in
    /// bytes: a title in Korean or with emoji takes 3-4 bytes a character, and a page that
    /// did not fit would hide its last entries, which no other page shows.
    fn queue_show(&self, page: NonZeroUsize) -> String {
        let Some(page) = self.player.queue_page(page, self.page_size) else {
            return "no such page, the queue is shorter".to_string();
        };
        let current = self.player.current();
        // Decoding ahead while the end of the current one plays, out of the queue already.
        let next = self.player.up_next();
        let footer = (page.count > 1).then(|| format!("page {}/{}", page.number, page.count));
        let count = usize::from(current.is_some())
            + usize::from(next.is_some())
            + page.entries.len().max(1);
        let share = chat::line_budget(count, footer.as_deref());
        let line = |prefix: String, track: &TrackRef| {
            let room = share.saturating_sub(prefix.len());
            prefix + &chat::fit(track.display_name(), QUEUE_TITLE_LEN, room)
        };

        let mut lines = Vec::new();
        if let Some(current) = current {
            lines.push(line("Now: ".into(), current.track));
        }
        if let Some(next) = next {
            lines.push(line("Next: ".into(), next));
        }
        if page.entries.is_empty() {
            lines.push("the queue is empty".to_string());
        }
        for (position, track) in &page.entries {
            lines.push(line(format!("{position}. "), track));
        }
        chat::message(lines, footer)
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
