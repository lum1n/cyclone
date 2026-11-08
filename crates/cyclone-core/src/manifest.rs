use serde::{Deserialize, Serialize};

use crate::platform::Platform;
use crate::profile::ComponentKind;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageManifest {
    pub schema_version: u32,
    /// RFC 3339 timestamp (UTC).
    pub captured_at: String,
    pub host_platform: Platform,
    pub host_arch: String,
    pub components: Vec<ManifestComponent>,
    pub secret_references: Vec<SecretReference>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestComponent {
    pub id: String,
    pub kind: ComponentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<String>,
    /// Logical path (`~/...`) used on the capture host.
    pub captured_path: String,
    pub content_sha256: String,
    pub file_count: usize,
    pub byte_size: u64,
    #[serde(default)]
    pub supported_platforms: Vec<Platform>,
}

impl ManifestComponent {
    pub fn path_for(&self, platform: Platform) -> Option<&str> {
        match platform {
            Platform::Linux => self.linux.as_deref(),
            Platform::MacOs => self.macos.as_deref(),
        }
    }

    pub fn supports(&self, platform: Platform) -> bool {
        self.supported_platforms.is_empty() || self.supported_platforms.contains(&platform)
    }
}

/// Secret *names* and providers only — never values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretReference {
    pub name: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_hint: Option<String>,
}

impl ImageManifest {
    pub fn stub_for_current_host() -> crate::error::Result<Self> {
        Ok(Self {
            schema_version: 1,
            captured_at: utc_now_rfc3339(),
            host_platform: Platform::current()?,
            host_arch: std::env::consts::ARCH.to_string(),
            components: Vec::new(),
            secret_references: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

pub fn utc_now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_epoch_rfc3339(dur.as_secs())
}

/// Minimal UTC RFC 3339 formatter (no external time crate).
fn format_epoch_rfc3339(secs: u64) -> String {
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

// Algorithm from Howard Hinnant (public domain), via common chrono implementations.
fn civil_from_days(z: u64) -> (u32, u32, u32) {
    let z = z as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m, d)
}

#[cfg(test)]
mod tests {
    use super::format_epoch_rfc3339;

    #[test]
    fn rfc3339_epoch() {
        assert_eq!(format_epoch_rfc3339(0), "1970-01-01T00:00:00Z");
    }
}
