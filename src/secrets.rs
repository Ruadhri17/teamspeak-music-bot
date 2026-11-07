//! Secrets from `secrets.toml` or `$CREDENTIALS_DIRECTORY/secrets`.
//!
//! - server password, home channel password, Tidal client id/secret as `SecretString`,
//! - refuse to start if the file is wider than 0600 or has another owner,
//! - Tidal session and TS identity stored in `state_dir` (0600, atomic writes).
