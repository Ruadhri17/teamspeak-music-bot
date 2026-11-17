//! Music sources.
//!
//! - `TrackRef`: a song in the queue, resolved on `!song add` and opened right before playing
//! - TODO: common `Source` interface: resolve a link, search (track/album/playlist), open a stream
//! - TODO: link detection with regexes (source and resource type), allow-list of hosts

pub mod local;
mod soundcloud;
mod spotify;
mod tidal;
mod youtube;

use std::path::PathBuf;

/// A song in the queue or playing. Holds no open file or stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRef {
    pub location: Location,
    /// Shown in `!queue show` and `!song current`.
    pub title: String,
}

/// Where a track comes from, one variant per source with its own identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// Relative to `music_dir`, checked again when opened.
    Local(PathBuf),
    // TODO: Tidal, Youtube, Spotify, Soundcloud with their own IDs.
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

#[cfg(test)]
mod tests;
