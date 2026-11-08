use std::path::Path;

use crate::bundle::ImageBundle;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectReport {
    pub path: String,
    pub schema_version: u32,
    pub host_platform: String,
    pub component_count: usize,
    pub secret_names: Vec<String>,
    pub warnings: Vec<String>,
    pub redacted: bool,
}

pub fn inspect(image_path: &Path) -> Result<InspectReport> {
    let bundle = ImageBundle::open(image_path)?;
    let secret_names: Vec<String> = bundle
        .manifest
        .secret_references
        .iter()
        .map(|s| s.name.clone())
        .collect();
    Ok(InspectReport {
        path: image_path.display().to_string(),
        schema_version: bundle.manifest.schema_version,
        host_platform: bundle.manifest.host_platform.as_str().to_string(),
        component_count: bundle.manifest.components.len(),
        secret_names,
        warnings: bundle.manifest.warnings.clone(),
        redacted: true,
    })
}
