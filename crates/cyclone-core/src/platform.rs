use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
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
        if let Ok(fake) = env::var("CYCLONE_FAKE_PLATFORM") {
            return fake.parse();
        }
        match env::consts::OS {
            "linux" => Ok(Platform::Linux),
            "macos" => Ok(Platform::MacOs),
            other => Err(CycloneError::UnsupportedPlatform(other.to_string())),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Linux => "linux",
            Platform::MacOs => "macos",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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

/// True when `name` is an executable on `PATH`.
pub fn command_on_path(name: &str) -> bool {
    let Ok(path) = env::var("PATH") else {
        return false;
    };
    path.split(':').any(|dir| {
        let candidate = Path::new(dir).join(name);
        candidate.is_file()
    })
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
