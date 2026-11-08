use std::path::Path;

use crate::bundle::ImageBundle;
use crate::error::Result;
use crate::manifest::ManifestComponent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectReport {
    pub path: String,
    pub schema_version: u32,
    pub host_platform: String,
    pub host_arch: String,
    pub captured_at: String,
    pub component_count: usize,
    pub components: Vec<ManifestComponent>,
    pub secret_names: Vec<String>,
    pub warnings: Vec<String>,
    pub aead: String,
    pub kdf: String,
    pub redacted: bool,
}

pub fn inspect(image_path: &Path, passphrase: &str) -> Result<InspectReport> {
    let bundle = ImageBundle::open(image_path)?;
    let inner = bundle.decrypt(passphrase)?;
    let secret_names: Vec<String> = inner
        .manifest
        .secret_references
        .iter()
        .map(|s| s.name.clone())
        .collect();
    Ok(InspectReport {
        path: image_path.display().to_string(),
        schema_version: inner.manifest.schema_version,
        host_platform: inner.manifest.host_platform.as_str().to_string(),
        host_arch: inner.manifest.host_arch.clone(),
        captured_at: inner.manifest.captured_at.clone(),
        component_count: inner.manifest.components.len(),
        components: inner.manifest.components.clone(),
        secret_names,
        warnings: inner.manifest.warnings.clone(),
        aead: bundle.header.aead.clone(),
        kdf: bundle.header.kdf.algorithm.clone(),
        redacted: true,
    })
}

/// Read the unencrypted envelope (KDF/AEAD labels only).
pub fn inspect_envelope(image_path: &Path) -> Result<(String, String)> {
    let bundle = ImageBundle::open(image_path)?;
    Ok((bundle.header.aead, bundle.header.kdf.algorithm))
}
