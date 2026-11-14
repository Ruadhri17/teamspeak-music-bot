//! Music sources.
//!
//! - common `Source` interface: resolve a link, search (track/album/playlist), open a stream,
//! - `TrackRef`: lightweight song description in the queue (no stream URL, since URLs expire),
//! - link detection with regexes (source and resource type), allow-list of hosts.

pub mod local;
mod soundcloud;
mod spotify;
mod tidal;
mod youtube;

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
