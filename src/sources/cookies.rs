//! The YouTube cookies of `sources.youtube.cookies_file`, for `yt-dlp`.
//!
//! `yt-dlp` rewrites its cookie file in place when it exits: two runs at once could read
//! a half-written file, and a run stopped while writing would cut it short. So the
//! configured file is only ever read, and each run gets its own copy in a new private
//! directory, removed together with the run. Cookies refreshed by a run are not kept,
//! cookies exported from a closed private window are not rotated (see docs/youtube.md).

use std::{
    fs::{OpenOptions, Permissions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::secrets::file::{FileError, read_private};

const FILE_NAME: &str = "cookies.txt";

/// Never contains the cookies, only the path.
#[derive(Debug, thiserror::Error)]
pub enum CookiesError {
    #[error(transparent)]
    File(#[from] FileError),

    #[error("{path} does not exist")]
    Missing { path: PathBuf },

    #[error("cannot copy the cookies for yt-dlp")]
    Copy(#[source] io::Error),
}

/// Checked at start, so a missing or unprotected file stops the bot instead of every lookup.
pub fn check(path: &Path) -> Result<(), CookiesError> {
    read(path).map(drop)
}

/// The cookies must be private to the bot's user, like `secrets.toml`.
fn read(path: &Path) -> Result<String, CookiesError> {
    read_private(path)?.ok_or_else(|| CookiesError::Missing {
        path: path.to_path_buf(),
    })
}

/// A private copy of the cookies for one `yt-dlp` run. Dropping it removes the copy with
/// its directory, so keep it until the process has exited.
#[derive(Debug)]
pub struct CookieCopy {
    /// Mode `0700`. A run stopped late may still write the file back, it can only do so
    /// while the directory exists, and only the bot's user can open it.
    dir: TempDir,
}

impl CookieCopy {
    /// Reads `source` again for every copy, so a new export is used without a restart.
    pub fn new(source: &Path) -> Result<Self, CookiesError> {
        let cookies = read(source)?;
        // `tempfile` follows the umask unless told otherwise, which usually gives `0755`.
        let dir = tempfile::Builder::new()
            .prefix("tsmusic-cookies-")
            .permissions(Permissions::from_mode(0o700))
            .tempdir()
            .map_err(CookiesError::Copy)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.path().join(FILE_NAME))
            .and_then(|mut file| file.write_all(cookies.as_bytes()))
            .map_err(CookiesError::Copy)?;
        Ok(Self { dir })
    }

    /// For `yt-dlp --cookies`.
    pub fn path(&self) -> PathBuf {
        self.dir.path().join(FILE_NAME)
    }
}
