//! Where the config file lives and everything that touches it on disk.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use super::{Config, ConfigError, ConfigResult, Sound, validate::validate};

/// Commented template written by `tsmusic init`. Path is relative to this source file.
pub(super) const TEMPLATE: &str = include_str!("../../config.example.toml");

/// The config file and its directory, created from `--config`.
#[derive(Debug, Clone)]
pub struct ConfigLocation {
    /// The `config.toml` file.
    file_path: PathBuf,
    /// Its directory, the base for relative paths in the config.
    base_dir: PathBuf,
}

impl ConfigLocation {
    /// Makes `file_path` absolute. The file does not need to exist yet.
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

        validate(&config).map_err(|source| ConfigError::Invalid {
            path: self.file_path.clone(),
            source,
        })?;

        self.resolve_paths(&mut config);
        Ok(config)
    }

    /// Makes relative paths (`state_dir`, `secrets_file`, event sound files, `music_dir`)
    /// relative to `base_dir`. Links in event sounds stay as they are.
    /// Bare program names (`ffmpeg`, `yt-dlp`) stay as they are and are looked up in `PATH`.
    /// `teamspeak.identity_file` is relative to `state_dir` and is joined where it is used.
    fn resolve_paths(&self, config: &mut Config) {
        self.resolve(&mut config.paths.state_dir);
        self.resolve(&mut config.paths.secrets_file);

        let events = &mut config.events;
        let sounds = [
            &mut events.on_connect.sound,
            &mut events.on_idle.sound,
            &mut events.on_alone.sound,
            &mut events.on_party.sound,
            &mut events.on_song_start.sound,
        ];
        let sound_files = sounds.into_iter().filter_map(|sound| match sound {
            Some(Sound::File(path)) => Some(path),
            _ => None,
        });
        for path in sound_files.chain(config.sources.local.music_dir.as_mut()) {
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

    // TODO:
    // - `set(&self, key, value)`: read the file from disk, change one key with `toml_edit`,
    //   validate the whole config, write it atomically with the helper of `secrets::file`
    //   generalised for the file mode (one atomic write in the bot, not two).
    // - `reload(&self, current)` (SIGHUP, `!bot config reload`): `load` + diff against `current`,
    //   keep the old config on error.
}
