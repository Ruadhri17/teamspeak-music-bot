//! Music sources.
//!
//! - `TrackRef`: a song in the queue, resolved on `!song add` and opened right before playing
//! - TODO: common `Source` interface: resolve a link, search (track/album/playlist), open a stream
//! - `parse_query`: link or search phrase, links checked against each source allow-list of hosts
//! - `music_dir`: the folder of the local source, used for lookups and for opening tracks

pub mod local;
mod soundcloud;
mod spotify;
pub mod youtube;

use std::path::{Path, PathBuf};

use url::Url;
use youtube::VideoId;

use crate::config::SourcesConfig;

/// A song in the queue or playing. Holds no open file or stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRef {
    pub location: Location,
    /// Shown in `!queue show` and `!song current`.
    pub title: String,
}

impl TrackRef {
    /// How the song is shown: `!song current`, `!queue show` and `{song}` in events.
    // TODO: `artist - title` for sources that know the artist (Spotify, Tidal, SoundCloud).
    pub fn display_name(&self) -> &str {
        &self.title
    }
}

/// What a lookup found: one track, or the tracks of a playlist in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Never empty.
    pub tracks: Vec<TrackRef>,
    /// `Some` when the query was a playlist.
    pub playlist: Option<Playlist>,
}

impl Found {
    pub fn track(track: TrackRef) -> Self {
        Self {
            tracks: vec![track],
            playlist: None,
        }
    }
}

/// A playlist the tracks of a `Found` come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub title: String,
    /// Videos in the whole playlist, also those over `max_playlist_len`. `None` when unknown.
    pub total: Option<usize>,
}

/// Where a track comes from, one variant per source with its own identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// Relative to `music_dir`, checked again when opened.
    Local(PathBuf),
    /// Any file on the disk. Only from the config (event sounds), never from the chat,
    /// which is limited to `Local`.
    File(PathBuf),

    Youtube(VideoId),
    // TODO: Tidal, Spotify, Soundcloud with their own IDs.
}

impl Location {
    /// The source for `{source}` in events.
    pub fn source_name(&self) -> &'static str {
        match self {
            Location::Local(_) => "local file",
            Location::File(_) => "file",
            Location::Youtube(_) => "YouTube",
        }
    }
}

/// What a search phrase looks for. Links carry their own type.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    #[default]
    Track,
    /// The whole album, in its original order.
    Album,
    /// A public playlist, in its original order.
    Playlist,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryType {
    Link(Url),
    Phrase(String),
}

/// Tells a link from a search phrase. Expects trimmed text with BBCode already removed.
///
/// A link without a scheme only counts for a known host.
/// A link with a scheme stays a link even for an unknown host, resolving rejects it later.
pub fn parse_query(query: &str) -> QueryType {
    if query.contains(char::is_whitespace) {
        return QueryType::Phrase(query.to_owned());
    }
    let url = match Url::parse(query) {
        Ok(url) => Some(url).filter(|url| matches!(url.scheme(), "http" | "https")),
        Err(url::ParseError::RelativeUrlWithoutBase) => Url::parse(&format!("https://{query}"))
            .ok()
            .filter(|url| url.host_str().is_some_and(is_known_host)),
        Err(_) => None,
    };
    match url {
        Some(url) => QueryType::Link(url),
        None => QueryType::Phrase(query.to_owned()),
    }
}

/// The folder of the local source. `None` in the config means the source is disabled,
/// the error is safe to show on the chat.
pub fn music_dir(sources: &SourcesConfig) -> Result<&Path, String> {
    sources
        .local
        .music_dir
        .as_deref()
        .ok_or_else(|| "the local source is disabled".to_string())
}

/// Hosts accepted in a link typed without `https://`.
fn is_known_host(host: &str) -> bool {
    youtube::HOSTS.contains(&host)
}

#[cfg(test)]
mod tests;
