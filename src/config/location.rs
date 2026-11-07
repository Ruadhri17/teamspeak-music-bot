//! Where the config file lives and everything that touches it on disk.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use super::{Config, ConfigError, ConfigResult};

/// Commented template written by `tsmusic init`. Path is relative to this source file.
pub(super) const TEMPLATE: &str = include_str!("../../config.toml.template");

/// Created once in `main` from `--config` and shared by everything that reads or writes the file.
#[derive(Debug, Clone)]
pub struct ConfigLocation {
    /// The `config.toml` file.
    file_path: PathBuf,
    /// Its directory, the base for relative paths in the config.
    base_dir: PathBuf,
}

impl ConfigLocation {
    /// Makes `file_path` absolute (`config.toml` -> `/current/dir/config.toml`), so `base_dir`
    /// does not depend on where the bot was started from. The file does not need to exist yet.
    pub fn new(file_path: PathBuf) -> ConfigResult<Self> {
        let invalid = |source| ConfigError::InvalidPath {
            path: file_path.clone(),
            source,
        };

        let file_path = std::path::absolute(&file_path).map_err(invalid)?;
        let base_dir = file_path
            .parent()
            .ok_or_else(|| {
                invalid(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a file path",
                ))
            })?
            .to_path_buf();

        Ok(Self {
            file_path,
            base_dir,
        })
    }

    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Writes `TEMPLATE` to the file. Refuses to overwrite an existing file unless `force`.
    pub fn init(&self, force: bool) -> ConfigResult<()> {
        let result = if force {
            fs::write(&self.file_path, TEMPLATE)
        } else {
            // `create_new` fails with `AlreadyExists` if the file is there.
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.file_path)
                .and_then(|mut file| file.write_all(TEMPLATE.as_bytes()))
        };

        result.map_err(|source| match source.kind() {
            io::ErrorKind::AlreadyExists => ConfigError::AlreadyExists {
                path: self.file_path.clone(),
            },
            _ => ConfigError::Write {
                path: self.file_path.clone(),
                source,
            },
        })
    }

    /// Reads, parses and validates the file. Resolves relative paths.
    pub fn load(&self) -> ConfigResult<Config> {
        let content = fs::read(&self.file_path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => ConfigError::NotFound {
                path: self.file_path.clone(),
            },
            _ => ConfigError::Read {
                path: self.file_path.clone(),
                source,
            },
        })?;

        let mut config: Config =
            toml::from_slice(&content).map_err(|source| ConfigError::Parse {
                path: self.file_path.clone(),
                source,
            })?;

        // TODO: validate
        self.resolve_paths(&mut config);
        Ok(config)
    }

    /// Makes relative paths (`state_dir`, event sounds, `music_dir`) relative to `base_dir`.
    /// Bare program names (`ffmpeg`, `yt-dlp`) stay as they are and are looked up in `PATH`.
    /// `teamspeak.identity_file` is relative to `state_dir` and is joined where it is used.
    fn resolve_paths(&self, config: &mut Config) {
        self.resolve(&mut config.paths.state_dir);

        for path in [
            &mut config.events.on_idle.sound,
            &mut config.events.on_alone.sound,
            &mut config.events.on_party.sound,
            &mut config.sources.local.music_dir,
        ]
        .into_iter()
        .flatten()
        {
            self.resolve(path);
        }

        self.resolve_program(&mut config.paths.ffmpeg_path);
        self.resolve_program(&mut config.sources.youtube.yt_dlp_path);
    }

    /// Joins relative path with `base_dir`. Absolute path stays as it is.
    fn resolve(&self, path: &mut PathBuf) {
        *path = self.base_dir.join(&*path);
    }

    /// Like `resolve`, but a bare name (`ffmpeg`) is left alone so it is looked up in `PATH`.
    fn resolve_program(&self, path: &mut PathBuf) {
        let has_dir = path.parent().is_some_and(|dir| !dir.as_os_str().is_empty());
        if has_dir {
            self.resolve(path);
        }
    }

    /// Writes `contents` to a temp file next to the config file, then renames it over the file.
    pub(super) fn write_atomic(&self, contents: &str) -> ConfigResult<()> {
        let temp_path = self.file_path.with_extension("toml.tmp");
        let mut temp = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)
            .map_err(|source| ConfigError::Write {
                path: temp_path.clone(),
                source,
            })?;

        temp.write_all(contents.as_bytes())
            .map_err(|source| ConfigError::Write {
                path: temp_path.clone(),
                source,
            })?;

        temp.sync_all().map_err(|source| ConfigError::Write {
            path: temp_path.clone(),
            source,
        })?;

        fs::rename(&temp_path, &self.file_path).map_err(|source| ConfigError::Write {
            path: temp_path,
            source,
        })?;

        Ok(())
    }

    // TODO:
    // - `set(&self, key, value)`: read the file from disk, change one key with `toml_edit`,
    //   validate the whole config, `write_atomic`.
    // - `reload(&self, current)` (SIGHUP, `!bot config reload`): `load` + diff against `current`,
    //   keep the old config on error.
}
