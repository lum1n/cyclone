use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{CycloneError, Result};
use crate::packages::PackageManager;
use crate::paths::logical_root_for;
use crate::platform::Platform;

/// User-editable capture configuration stored under the Cyclone config directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub schema_version: u32,
    #[serde(default)]
    pub components: Vec<ProfileComponent>,
    #[serde(default)]
    pub secret_references: Vec<crate::manifest::SecretReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentKind {
    Dotfile,
    ConfigTree,
    ToolManifest,
}

impl ComponentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ComponentKind::Dotfile => "dotfile",
            ComponentKind::ConfigTree => "config-tree",
            ComponentKind::ToolManifest => "tool-manifest",
        }
    }
}

/// Why a discovered path is left out of capture unless the profile opts in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OptInReason {
    SshPrivateKey,
    Keychain,
    BrowserData,
    LargeCache,
}

impl OptInReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SshPrivateKey => "ssh-private-key",
            Self::Keychain => "keychain",
            Self::BrowserData => "browser-data",
            Self::LargeCache => "large-cache",
        }
    }
}

impl std::fmt::Display for OptInReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One capture unit with per-OS logical paths (`~/...`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileComponent {
    pub id: String,
    pub kind: ComponentKind,
    #[serde(default = "default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<String>,
    #[serde(default)]
    pub supported_platforms: Vec<Platform>,
    /// Native package manager when this component is a declaration dump.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<PackageManager>,
    /// Set when the scanner found this path but left it out by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opt_in: Option<OptInReason>,
}

fn default_true() -> bool {
    true
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            components: Vec::new(),
            secret_references: Vec::new(),
            notes: None,
        }
    }
}

impl ProfileComponent {
    pub fn path_for(&self, platform: Platform) -> Option<&str> {
        match platform {
            Platform::Linux => self.linux.as_deref(),
            Platform::MacOs => self.macos.as_deref(),
        }
    }

    pub fn supports(&self, platform: Platform) -> bool {
        self.supported_platforms.is_empty() || self.supported_platforms.contains(&platform)
    }

    pub fn logical_root(&self, platform: Platform) -> Option<String> {
        logical_root_for(self.linux.as_deref(), self.macos.as_deref(), platform)
    }
}

impl Profile {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let profile: Profile = toml::from_str(&raw)?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
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

    pub fn included(&self) -> impl Iterator<Item = &ProfileComponent> {
        self.components.iter().filter(|c| c.include)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version == 0 {
            return Err(CycloneError::InvalidManifest(
                "profile schema_version must be >= 1".into(),
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for c in &self.components {
            if c.id.is_empty() {
                return Err(CycloneError::InvalidManifest(
                    "component id must not be empty".into(),
                ));
            }
            if !ids.insert(c.id.as_str()) {
                return Err(CycloneError::InvalidManifest(format!(
                    "duplicate component id `{}`",
                    c.id
                )));
            }
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
        let base = directories::ProjectDirs::from("dev", "cyclone", "cyclone")
            .ok_or_else(|| CycloneError::User("cannot resolve config directory".into()))?;
        let config_dir = base.config_dir().to_path_buf();
        let profile_path = config_dir.join("profile.toml");
        Ok(Self {
            config_dir,
            profile_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_toml() {
        let profile = Profile {
            schema_version: 1,
            components: vec![ProfileComponent {
                id: "gitconfig".into(),
                kind: ComponentKind::Dotfile,
                include: true,
                linux: Some("~/.gitconfig".into()),
                macos: Some("~/.gitconfig".into()),
                supported_platforms: vec![Platform::Linux, Platform::MacOs],
                package_manager: None,
                opt_in: None,
            }],
            secret_references: vec![],
            notes: Some("test".into()),
        };
        let raw = toml::to_string_pretty(&profile).unwrap();
        let back: Profile = toml::from_str(&raw).unwrap();
        assert_eq!(profile, back);
    }
}
