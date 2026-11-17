//! Files with sensitive content: refuses files readable by others or owned by another user,
//! writes atomically (temp file created with the final permissions, then renamed).

use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("cannot read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{path} is accessible by other users (mode {mode:o}), run: chmod 600 {path}")]
    Permissions { path: PathBuf, mode: u32 },

    #[error("{path} is owned by another user")]
    Owner { path: PathBuf },

    #[error("cannot write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Reads a file that must be private to the bot's user. `None` when it does not exist.
pub fn read_private(path: &Path) -> Result<Option<String>, FileError> {
    let read_error = |source| FileError::Read {
        path: path.to_path_buf(),
        source,
    };

    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(read_error(err)),
    };

    // Checked on the opened file, not the path, so the file cannot be swapped in between.
    let metadata = file.metadata().map_err(read_error)?;
    let mode = metadata.mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(FileError::Permissions {
            path: path.to_path_buf(),
            mode,
        });
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(FileError::Owner {
            path: path.to_path_buf(),
        });
    }

    io::read_to_string(file).map(Some).map_err(read_error)
}

/// Writes `contents` atomically with mode `0600`. Missing parent directories are created
/// with mode `0700`, existing ones are left as they are.
pub fn write_private(path: &Path, contents: &str) -> Result<(), FileError> {
    let write_error = |source| FileError::Write {
        path: path.to_path_buf(),
        source,
    };

    let Some(name) = path.file_name() else {
        return Err(write_error(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a file path",
        )));
    };
    // `parent` of a bare file name is an empty path, which means the current directory.
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(write_error)?;

    // Same directory as the target, `rename` is atomic only within one file system.
    let mut temp_name = name.to_os_string();
    temp_name.push(".tmp");
    let temp = dir.join(temp_name);

    // A temp file left by a crash would make `create_new` fail.
    match fs::remove_file(&temp) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(write_error(err)),
    }

    let result = write_temp(&temp, contents).and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        // Best effort cleanup, the original error is the one worth reporting.
        let _ = fs::remove_file(&temp);
    }
    result.map_err(write_error)?;

    // Makes the rename itself durable, not only the file contents.
    File::open(dir)
        .and_then(|dir| dir.sync_all())
        .map_err(write_error)
}

/// Created with `0600` from the start, so the contents are never readable by others.
fn write_temp(path: &Path, contents: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()
}

/// Where a `toml` error is, as `line N`. The error itself is not shown, it may quote the file.
pub fn error_line(text: &str, err: &toml::de::Error) -> String {
    match err.span() {
        Some(span) => {
            let line = text[..span.start.min(text.len())].matches('\n').count() + 1;
            format!("line {line}")
        }
        None => "unknown line".to_string(),
    }
}
