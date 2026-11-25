//! `!song add`: lookups run in the background and their tracks join the queue in the order
//! of the commands, not in the order the lookups finish.

use std::sync::Arc;

use tokio::task::{Id, JoinError};
use tracing::warn;

use super::Bot;
use crate::{
    chat,
    config::{SourceKind, SourcesConfig},
    player::Enqueued,
    sources::{Found, QueryType, local, music_dir, youtube},
    teamspeak::ReplyTo,
};

/// A `!song add` whose lookup runs in `Bot::lookups`, kept in the order of the commands.
#[derive(Debug)]
pub(super) struct Pending {
    /// The lookup task, matched with the ID that `join_next_with_id` returns.
    task: Id,
    /// Where an error goes, the place the command came from.
    reply_to: ReplyTo,
    /// `None` while the lookup runs.
    result: Option<Result<Found, String>>,
}

impl Bot {
    /// `!song add`. Starts the lookup, `lookup_done` queues the track in the order of the commands.
    pub(super) fn add(&mut self, query: QueryType, source: Option<SourceKind>, reply_to: ReplyTo) {
        let sources = Arc::clone(&self.sources);
        let task = self.lookups.spawn(resolve(sources, query, source)).id();
        self.pending.push_back(Pending {
            task,
            reply_to,
            result: None,
        });
    }

    /// Stores the result of a lookup, then queues every finished lookup at the front of
    /// `pending`. Errors and the summary of a playlist go to the place the command came from.
    pub(super) fn lookup_done(&mut self, joined: Result<(Id, Result<Found, String>), JoinError>) {
        let (task, result) = match joined {
            Ok(done) => done,
            // Aborted by `!queue clear`, which also removed the entry.
            Err(err) if err.is_cancelled() => return,
            Err(err) => {
                warn!("lookup task panicked: {err}");
                (err.id(), Err("the lookup failed".to_string()))
            }
        };
        if let Some(entry) = self.pending.iter_mut().find(|entry| entry.task == task) {
            entry.result = Some(result);
        }

        while let Some(entry) = self.pending.pop_front_if(|entry| entry.result.is_some()) {
            let found = match entry.result.expect("only finished entries are taken") {
                Ok(found) => found,
                Err(text) => {
                    self.say(entry.reply_to, text);
                    continue;
                }
            };
            if let Some(text) = self.enqueue_found(found) {
                self.say(entry.reply_to, text);
            }
        }
    }

    /// Queues the tracks. Returns the message for the sender: a full queue, or the summary
    /// of a playlist. A single track that fits is silent.
    fn enqueue_found(&mut self, Found { tracks, playlist }: Found) -> Option<String> {
        let count = tracks.len();
        let Enqueued { added, full } = self.player.enqueue(tracks);
        self.report();

        let total = playlist
            .as_ref()
            .and_then(|playlist| playlist.total)
            .map_or(count, |total| total.max(count));
        let summary = || match &playlist {
            Some(playlist) => added_from(added, total, &playlist.title),
            None => format!("added {added} of {total} songs"),
        };
        match full {
            Some(full) if added == 0 => Some(full.to_string()),
            Some(full) => Some(format!("{full}, {}", summary())),
            None => playlist.is_some().then(summary),
        }
    }
}

/// Turns a query into a track with the chosen source, or the default one from the config.
async fn resolve(
    sources: Arc<SourcesConfig>,
    query: QueryType,
    source: Option<SourceKind>,
) -> Result<Found, String> {
    match source.unwrap_or(sources.default) {
        SourceKind::Tidal | SourceKind::Spotify | SourceKind::Soundcloud => {
            Err("this source is not supported yet".to_string())
        }
        SourceKind::Youtube => youtube::resolve(&sources.youtube, &query, sources.max_playlist_len)
            .await
            .map_err(|err| err.to_string()),
        SourceKind::Local => match query {
            QueryType::Phrase(path) => {
                let track =
                    local::resolve(music_dir(&sources)?, &path).map_err(|err| err.to_string())?;
                Ok(Found::track(track))
            }
            QueryType::Link(_) => Err("the local source takes a path, not a link".to_string()),
        },
    }
}

/// `added 3 songs from X`, or `added 100 of 182 songs from X` when part of the playlist
/// was left out. The title comes from outside the bot, so it is escaped.
fn added_from(added: usize, total: usize, title: &str) -> String {
    let songs = if total == 1 { "song" } else { "songs" };
    let title = chat::escape(title);
    if added < total {
        format!("added {added} of {total} {songs} from {title}")
    } else {
        format!("added {added} {songs} from {title}")
    }
}
