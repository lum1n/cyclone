use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::error::{CycloneError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    MacOs,
}

impl Platform {
    pub fn current() -> Result<Self> {
        match std::env::consts::OS {
            "linux" => Ok(Platform::Linux),
            "macos" => Ok(Platform::MacOs),
            other => Err(CycloneError::UnsupportedPlatform(other.to_string())),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Linux => "linux",
            Platform::MacOs => "macos",
        }
    }
}

impl FromStr for Platform {
    type Err = CycloneError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "linux" => Ok(Platform::Linux),
            "macos" | "darwin" => Ok(Platform::MacOs),
            _ => Err(CycloneError::UnsupportedPlatform(s.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_platform() {
        assert_eq!(Platform::from_str("linux").unwrap(), Platform::Linux);
        assert_eq!(Platform::from_str("darwin").unwrap(), Platform::MacOs);
    }
}
