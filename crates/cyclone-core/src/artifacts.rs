//! Encrypted payload schema stored inside an image bundle.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Version 1 encrypted payload: captured file blobs keyed by manifest component name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapturedPayload {
    pub schema_version: u32,
    pub artifacts: Vec<CapturedArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapturedArtifact {
    /// Stable id matching `ManifestComponent::name` where possible.
    pub name: String,
    pub source_path: PathBuf,
    pub kind: ArtifactKind,
    /// Raw bytes for [`ArtifactKind::File`]; newline-separated relative paths for listings.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    File,
    DirectoryListing,
}

impl CapturedPayload {
    pub fn new(artifacts: Vec<CapturedArtifact>) -> Self {
        Self {
            schema_version: 1,
            artifacts,
        }
    }
}
