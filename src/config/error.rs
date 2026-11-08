use std::path::PathBuf;

use super::validate::ValidationError;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid config path {path}")]
    InvalidPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("config file not found: {path}, create it with `tsmusic init`")]
    NotFound { path: PathBuf },

    #[error("{path} already exists, use --force to overwrite it")]
    AlreadyExists { path: PathBuf },

    #[error("cannot read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid config file {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid config file {path}")]
    Invalid {
        path: PathBuf,
        #[source]
        source: ValidationError,
    },
}

pub type ConfigResult<T> = Result<T, ConfigError>;
