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

/// Distro identity from `/etc/os-release` (Linux) or a macOS stub. Never assumed to be Ubuntu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Distro {
    pub id: String,
    #[serde(default)]
    pub id_like: Vec<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
}

impl Distro {
    pub fn macos() -> Self {
        Self {
            id: "macos".into(),
            id_like: Vec::new(),
            name: "macOS".into(),
            version_id: None,
        }
    }

    pub fn unknown_linux() -> Self {
        Self {
            id: "linux".into(),
            id_like: Vec::new(),
            name: "Linux".into(),
            version_id: None,
        }
    }

    pub fn from_id(id: &str) -> Self {
        let id_l = id.to_ascii_lowercase();
        let (name, id_like): (String, Vec<String>) = match id_l.as_str() {
            "macos" | "darwin" => ("macOS".into(), Vec::new()),
            "ubuntu" => ("Ubuntu".into(), vec!["debian".into()]),
            "debian" => ("Debian".into(), Vec::new()),
            "linuxmint" | "pop" | "elementary" | "raspbian" => {
                ("Debian-like".into(), vec!["debian".into()])
            }
            "fedora" => ("Fedora Linux".into(), Vec::new()),
            "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "nobara" | "mageia" => {
                ("RHEL-like".into(), vec!["rhel".into(), "fedora".into()])
            }
            "arch" | "archlinux" => ("Arch Linux".into(), Vec::new()),
            "manjaro" | "endeavouros" | "cachyos" | "garuda" => {
                ("Arch-like".into(), vec!["arch".into()])
            }
            "alpine" => ("Alpine Linux".into(), Vec::new()),
            "opensuse-tumbleweed" | "opensuse-leap" | "opensuse" | "sles" | "suse" => {
                ("openSUSE".into(), vec!["suse".into()])
            }
            _ => (id_l.clone(), Vec::new()),
        };
        let id = match id_l.as_str() {
            "darwin" => "macos".into(),
            "opensuse" => "opensuse-tumbleweed".into(),
            _ => id_l,
        };
        Self {
            id,
            id_like,
            name,
            version_id: None,
        }
    }

    pub fn detect_linux() -> Self {
        if let Ok(path) = env::var("CYCLONE_OS_RELEASE") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                return Self::from_os_release(&text);
            }
        }
        for candidate in ["/etc/os-release", "/usr/lib/os-release"] {
            if let Ok(text) = std::fs::read_to_string(candidate) {
                return Self::from_os_release(&text);
            }
        }
        Self::unknown_linux()
    }

    pub fn from_os_release(text: &str) -> Self {
        let mut id = String::new();
        let mut id_like = Vec::new();
        let mut name = String::new();
        let mut version_id = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, raw)) = line.split_once('=') else {
                continue;
            };
            let value = unquote(raw);
            match key {
                "ID" => id = value.to_ascii_lowercase(),
                "ID_LIKE" => {
                    id_like = value
                        .split_whitespace()
                        .map(|s| s.to_ascii_lowercase())
                        .collect();
                }
                "NAME" => name = value,
                "VERSION_ID" => version_id = Some(value),
                _ => {}
            }
        }
        if id.is_empty() {
            return Self::unknown_linux();
        }
        if name.is_empty() {
            name = id.clone();
        }
        Self {
            id,
            id_like,
            name,
            version_id,
        }
    }

    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.id.as_str()).chain(self.id_like.iter().map(String::as_str))
    }

    pub fn has_token(&self, token: &str) -> bool {
        self.tokens().any(|t| t == token)
    }
}

fn unquote(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(inner) = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
    {
        inner.to_string()
    } else {
        raw.to_string()
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

    #[test]
    fn os_release_fedora_is_not_ubuntu() {
        let fedora = Distro::from_os_release("NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=41\n");
        assert_eq!(fedora.id, "fedora");
        assert!(!fedora.has_token("ubuntu"));
        let ubuntu = Distro::from_os_release("ID=ubuntu\nID_LIKE=debian\nNAME=\"Ubuntu\"\n");
        assert!(ubuntu.has_token("debian"));
        assert_eq!(ubuntu.id, "ubuntu");
    }
}
