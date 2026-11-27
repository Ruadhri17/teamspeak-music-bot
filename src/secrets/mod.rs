//! Secrets and bot state files.
//!
//! - `mod.rs`: `Secrets` from `secrets.toml` or `$CREDENTIALS_DIRECTORY/secrets`
//!   (server password, home channel password, Tidal client id/secret as `SecretString`)
//! - `identity`: the TS identity in `state_dir`, created on the first start
//! - `file`: owner and `0600` checks, atomic writes with restricted permissions

pub mod file;
pub mod identity;

use std::{
    fs,
    path::{Path, PathBuf},
};

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tracing::info;

use file::FileError;

/// Name of the credential in `LoadCredentialEncrypted=secrets:...`.
const CREDENTIAL_NAME: &str = "secrets";

/// Never contains secret values, `toml` errors are not kept since they quote the file.
#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error(transparent)]
    File(#[from] FileError),

    #[error("cannot read the systemd credential {path}")]
    Credential {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid secrets file {path}, {location}")]
    Parse { path: PathBuf, location: String },
}

const EMPTY: &str = "must not be empty, remove the line instead";

/// A secret value that is never empty, checked while parsing.
#[derive(Deserialize, Debug)]
#[serde(try_from = "SecretString")]
pub struct Secret(SecretString);

impl TryFrom<SecretString> for Secret {
    type Error = &'static str;

    fn try_from(value: SecretString) -> Result<Self, Self::Error> {
        if value.expose_secret().trim().is_empty() {
            Err(EMPTY)
        } else {
            Ok(Self(value))
        }
    }
}

impl ExposeSecret<str> for Secret {
    fn expose_secret(&self) -> &str {
        self.0.expose_secret()
    }
}

/// Values entered by the admin. `Debug` is safe, `SecretString` prints `[REDACTED]`.
#[derive(Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct Secrets {
    pub teamspeak: TeamspeakSecrets,
    pub tidal: TidalSecrets,
}

#[derive(Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct TeamspeakSecrets {
    pub server_password: Option<Secret>,
    pub channel_password: Option<Secret>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct TidalSecrets {
    pub client_id: Option<Secret>,
    pub client_secret: Option<Secret>,
}

impl Secrets {
    /// Reads `$CREDENTIALS_DIRECTORY/secrets` under systemd, otherwise `path`,
    /// which must be private to the bot's user. A missing `path` gives empty secrets.
    pub fn load(path: &Path) -> Result<Self, SecretsError> {
        let credentials = std::env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from);
        Self::load_from(credentials.as_deref(), path)
    }

    /// `load` with the credentials directory passed in, so tests do not touch the environment.
    fn load_from(credentials: Option<&Path>, path: &Path) -> Result<Self, SecretsError> {
        if let Some(dir) = credentials {
            // systemd keeps credentials in a private read-only mount, no permission check.
            let path = dir.join(CREDENTIAL_NAME);
            let text = fs::read_to_string(&path).map_err(|source| SecretsError::Credential {
                path: path.clone(),
                source,
            })?;
            return Self::parse(&text, &path);
        }

        match file::read_private(path)? {
            Some(text) => Self::parse(&text, path),
            None => {
                info!(
                    "no secrets file at {}, running without secrets",
                    path.display()
                );
                Ok(Self::default())
            }
        }
    }

    fn parse(text: &str, path: &Path) -> Result<Self, SecretsError> {
        toml::from_str(text).map_err(|err| {
            let line = file::error_line(text, &err);
            // Only our own message is shown, other `toml` messages may quote the value.
            let location = if err.message() == EMPTY {
                format!("{line}: {EMPTY}")
            } else {
                line
            };
            SecretsError::Parse {
                path: path.to_path_buf(),
                location,
            }
        })
    }
}

#[cfg(test)]
mod tests;
