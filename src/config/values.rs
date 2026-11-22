//! Value types with built-in limits.
//!
//! Checked while serde parses the file (`try_from`), so an error points to its line.

use serde::{Deserialize, Serialize};

/// Opus bitrate in kb/s, 32-192.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "u32", into = "u32")]
pub struct Bitrate(u32);

impl Bitrate {
    pub const MIN: u32 = 32;
    pub const MAX: u32 = 192;

    pub const fn new(kbps: u32) -> Option<Self> {
        if kbps >= Self::MIN && kbps <= Self::MAX {
            Some(Self(kbps))
        } else {
            None
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for Bitrate {
    type Error = String;

    fn try_from(kbps: u32) -> Result<Self, Self::Error> {
        Self::new(kbps).ok_or_else(|| {
            format!(
                "bitrate must be between {} and {} kb/s",
                Self::MIN,
                Self::MAX
            )
        })
    }
}

impl From<Bitrate> for u32 {
    fn from(bitrate: Bitrate) -> Self {
        bitrate.0
    }
}

/// Opus encoder complexity, 0-10.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "u8", into = "u8")]
pub struct Complexity(u8);

impl Complexity {
    pub const MAX: u8 = 10;

    pub const fn new(level: u8) -> Option<Self> {
        if level <= Self::MAX {
            Some(Self(level))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Complexity {
    type Error = String;

    fn try_from(level: u8) -> Result<Self, Self::Error> {
        Self::new(level).ok_or_else(|| format!("complexity must be between 0 and {}", Self::MAX))
    }
}

impl From<Complexity> for u8 {
    fn from(complexity: Complexity) -> Self {
        complexity.0
    }
}

/// Volume in percent, 0-100.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(try_from = "u8", into = "u8")]
pub struct Volume(u8);

impl Volume {
    pub const MAX: u8 = 100;
    /// 100 %, the track as it is.
    pub const FULL: Self = Self(Self::MAX);

    pub const fn new(percent: u8) -> Option<Self> {
        if percent <= Self::MAX {
            Some(Self(percent))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Volume {
    type Error = String;

    fn try_from(percent: u8) -> Result<Self, Self::Error> {
        Self::new(percent).ok_or_else(|| format!("volume must be between 0 and {}", Self::MAX))
    }
}

impl From<Volume> for u8 {
    fn from(volume: Volume) -> Self {
        volume.0
    }
}
