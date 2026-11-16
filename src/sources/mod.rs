//! Music sources.
//!
//! - `TrackRef`: lightweight song description in the queue (no stream URL, since URLs expire),
//!   resolved on `!song add` and opened only right before playing
//! - TODO: common `Source` interface: resolve a link, search (track/album/playlist), open a stream
//! - TODO: link detection with regexes (source and resource type), allow-list of hosts

pub mod local;
mod soundcloud;
mod spotify;
mod tidal;
mod youtube;

use std::path::PathBuf;

/// A song waiting in the queue or playing. Holds no open file or stream,
/// so a long queue costs nothing and nothing expires while it waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRef {
    pub location: Location,
    /// Shown in `!queue show` and `!song current`.
    pub title: String,
}

/// Where a track comes from and how its source finds it again.
/// One variant per source, so each carries the identifier type that source uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// Relative to `music_dir`, checked again when opened.
    Local(PathBuf),
    // TODO: Tidal, Youtube, Spotify, Soundcloud with their own IDs.
}

/// What a search phrase looks for. A link carries its own type, so this only applies to phrases.
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
