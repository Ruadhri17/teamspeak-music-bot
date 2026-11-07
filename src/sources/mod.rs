//! Music sources.
//!
//! - common `Source` interface: resolve a link, search (track/album/playlist), open a stream,
//! - `TrackRef`: lightweight song description in the queue (no stream URL, since URLs expire),
//! - link detection with regexes (source and resource type), allow-list of hosts.

mod local;
mod soundcloud;
mod spotify;
mod tidal;
mod youtube;
