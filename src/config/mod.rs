//! Bot configuration loaded from `config.toml`.
//!
//! - `model`: sections, fields and defaults
//! - `values`: value types with built-in limits
//! - `location`: `ConfigLocation`, init, load and save of the file on disk
//! - `validate`: value ranges and other rules serde cannot check
//! - `error`: `ConfigError` for the whole module

mod error;
mod location;
mod model;
mod validate;
mod values;

pub use error::{ConfigError, ConfigResult};
pub use location::ConfigLocation;
pub use model::*;
pub use values::{Bitrate, Complexity, Volume};

#[cfg(test)]
mod tests;
