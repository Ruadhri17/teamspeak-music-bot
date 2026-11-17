//! TeamSpeak identity stored in `state_dir/identity_file`.
//! Created and saved on the first start, so the bot keeps its UID and server permissions.

use std::path::{Path, PathBuf};

use tracing::info;
use tsclientlib::Identity;

use super::file::{self, FileError};

/// Never contains the private key.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error(transparent)]
    File(#[from] FileError),

    #[error("invalid identity file {path}, {location}")]
    Parse { path: PathBuf, location: String },

    #[error("cannot serialize the identity")]
    Serialize,
}

/// Loads the identity from `path`, or creates and saves a new one when the file does not exist.
/// A broken file is an error, not a reason to create a new identity, since that changes the UID.
pub fn load_or_create(path: &Path) -> Result<Identity, IdentityError> {
    if let Some(text) = file::read_private(path)? {
        let identity: Identity = toml::from_str(&text).map_err(|err| IdentityError::Parse {
            path: path.to_path_buf(),
            location: file::error_line(&text, &err),
        })?;
        info!("loaded TeamSpeak identity {}", uid(&identity));
        return Ok(identity);
    }

    let identity = Identity::create();
    save(path, &identity)?;
    info!(
        "created TeamSpeak identity {} in {}",
        uid(&identity),
        path.display()
    );
    Ok(identity)
}

/// Saves `identity` to `path` with mode `0600`.
pub fn save(path: &Path, identity: &Identity) -> Result<(), IdentityError> {
    // The `toml` error is dropped, it may contain the serialized key.
    let text = toml::to_string(identity).map_err(|_| IdentityError::Serialize)?;
    file::write_private(path, &text)?;
    Ok(())
}

/// The public UID, as shown in the TeamSpeak client.
pub fn uid(identity: &Identity) -> String {
    identity.key().to_pub().get_uid()
}
