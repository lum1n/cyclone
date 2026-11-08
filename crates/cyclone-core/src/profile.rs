use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{CycloneError, Result};
use crate::platform::Platform;

/// User-editable capture configuration stored under the Cyclone config directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub schema_version: u32,
    pub capture_roots: Vec<PathBuf>,
    pub tool_declarations: Vec<ToolDeclaration>,
    pub secret_references: Vec<crate::manifest::SecretReference>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDeclaration {
    pub name: String,
    pub kind: String,
    pub pin: Option<String>,
    #[serde(default)]
    pub supported_platforms: Vec<Platform>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            capture_roots: Vec::new(),
            tool_declarations: Vec::new(),
            secret_references: Vec::new(),
            notes: None,
        }
    }
}

impl Profile {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let profile: Profile = toml::from_str(&raw)?;
        Ok(profile)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(path, raw)?;
        Ok(())
    }

    pub fn exists(path: &Path) -> bool {
        path.is_file()
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version == 0 {
            return Err(CycloneError::InvalidManifest(
                "profile schema_version must be >= 1".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ProfilePaths {
    pub config_dir: PathBuf,
    pub profile_path: PathBuf,
}

impl ProfilePaths {
    pub fn discover() -> Result<Self> {
        let base =
            directories::ProjectDirs::from("dev", "cyclone", "cyclone").ok_or_else(|| {
                CycloneError::Other(anyhow::anyhow!("cannot resolve config directory"))
            })?;
        let config_dir = base.config_dir().to_path_buf();
        let profile_path = config_dir.join("profile.toml");
        Ok(Self {
            config_dir,
            profile_path,
        })
    }
}
