//! Encrypted payload schema stored inside an image bundle.

use serde::{Deserialize, Serialize};

/// Version 1 encrypted payload: captured file blobs keyed by profile component id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapturedPayload {
    pub schema_version: u32,
    pub artifacts: Vec<CapturedArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapturedArtifact {
    pub component_id: String,
    /// Path relative to the component root using `/` separators. Empty for a single-file component.
    pub relative_path: String,
    pub kind: ArtifactKind,
    #[serde(with = "b64")]
    pub bytes: Vec<u8>,
    /// Unix mode bits (e.g. 0o644).
    pub mode: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    File,
}

impl CapturedPayload {
    pub fn new(artifacts: Vec<CapturedArtifact>) -> Self {
        Self {
            schema_version: 1,
            artifacts,
        }
    }

    pub fn files_for(&self, component_id: &str) -> Vec<&CapturedArtifact> {
        self.artifacts
            .iter()
            .filter(|a| a.component_id == component_id)
            .collect()
    }
}

mod b64 {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}
