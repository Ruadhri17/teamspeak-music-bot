//! `local` source: files from `sources.local.music_dir`. Never leaves that directory
//! (canonicalize, check prefix). Search by file name and tags, "album" = subdirectory.

use std::{fs::File, io, path::Path};

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

/// `path` is relative to `music_dir`, as typed in `!song add`.
pub fn open(music_dir: &Path, path: &str) -> Result<AudioInput, LocalError> {
    // Both sides canonical (`..`, symlinks and relative parts resolved),
    // so the prefix check below cannot be fooled.
    let music_dir = music_dir.canonicalize()?;
    let path = music_dir.join(path).canonicalize()?;

    // Outside answers the same as missing, so the chat cannot probe which files exist on the host.
    if !path.starts_with(&music_dir) || !path.is_file() {
        return Err(LocalError::NotFound);
    }
    Ok(Box::new(File::open(path)?))
}
