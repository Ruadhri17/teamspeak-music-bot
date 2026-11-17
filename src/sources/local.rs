//! `local` source: files from `sources.local.music_dir`, never outside it.
//! TODO: search by file name and tags, "album" = subdirectory.

use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

use super::{Location, TrackRef};
use crate::audio::AudioInput;

/// Shown on the chat, so the messages never contain system paths.
#[derive(Debug, thiserror::Error)]
pub enum LocalError {
    #[error("no such file in the music directory")]
    NotFound,

    #[error("cannot open the file")]
    Io(#[source] io::Error),
}

impl From<io::Error> for LocalError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::NotFound => LocalError::NotFound,
            _ => LocalError::Io(err),
        }
    }
}

/// Checks the path and describes the track, opens nothing.
/// `query` is relative to `music_dir`.
pub fn resolve(music_dir: &Path, query: &str) -> Result<TrackRef, LocalError> {
    let (full, relative) = inside(music_dir, Path::new(query))?;
    let title = match full.file_stem() {
        Some(stem) => stem.to_string_lossy().into_owned(),
        None => query.to_string(),
    };
    Ok(TrackRef {
        location: Location::Local(relative),
        title,
    })
}

/// Opens the file right before playing. Checks the path again,
/// it may have become a symlink outside `music_dir` while queued.
pub fn open(music_dir: &Path, path: &Path) -> Result<AudioInput, LocalError> {
    let (full, _) = inside(music_dir, path)?;
    Ok(Box::new(File::open(full)?))
}

/// Returns the canonical full path and the path relative to `music_dir`,
/// or `NotFound` when `path` leaves `music_dir` or is not a file.
fn inside(music_dir: &Path, path: &Path) -> Result<(PathBuf, PathBuf), LocalError> {
    // Both sides are canonical (`..`, symlinks and relative parts resolved),
    // so the prefix check below is reliable.
    let music_dir = music_dir.canonicalize()?;
    let full = music_dir.join(path).canonicalize()?;

    // `strip_prefix` fails exactly when `full` is outside `music_dir`.
    // Outside answers the same as missing, so the chat cannot probe which files exist on the host.
    let relative = full
        .strip_prefix(&music_dir)
        .map_err(|_| LocalError::NotFound)?
        .to_path_buf();
    if !full.is_file() {
        return Err(LocalError::NotFound);
    }
    Ok((full, relative))
}
