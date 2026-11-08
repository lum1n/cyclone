use std::path::{Path, PathBuf};

use crate::artifacts::{ArtifactKind, CapturedPayload};
use crate::bundle::ImageBundle;
use crate::crypto::EncryptionKey;
use crate::error::Result;
use crate::platform::Platform;

#[derive(Debug, Clone)]
pub struct RestoreOptions<'a> {
    pub image: &'a Path,
    pub encryption_key: Option<&'a EncryptionKey>,
    /// Destination parent for file artifacts (defaults to original paths when `None`).
    pub target_root: Option<PathBuf>,
    pub dry_run: bool,
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub dry_run: bool,
    pub planned_actions: Vec<String>,
    pub warnings: Vec<String>,
    pub applied_files: usize,
}

pub fn restore(opts: RestoreOptions<'_>) -> Result<RestoreReport> {
    let bundle = ImageBundle::open(opts.image)?;
    let current = Platform::current()?;
    let mut warnings = bundle.manifest.warnings.clone();
    let mut planned = Vec::new();
    let mut applied_files = 0usize;

    for component in &bundle.manifest.components {
        let compatible = component.supported_platforms.is_empty()
            || component.supported_platforms.contains(&current);
        if !compatible {
            warnings.push(format!(
                "component `{}` not supported on {}",
                component.name,
                current.as_str()
            ));
        }
        planned.push(format!(
            "restore {} ({})",
            component.source_path, component.kind
        ));
    }

    if opts.dry_run {
        return Ok(RestoreReport {
            dry_run: true,
            planned_actions: planned,
            warnings,
            applied_files: 0,
        });
    }

    let key = opts.encryption_key.ok_or_else(|| {
        crate::error::CycloneError::Crypto(
            "encryption key required for restore (set CYCLONE_PASSPHRASE)".into(),
        )
    })?;
    let plaintext = bundle.read_encrypted_payload(key)?;
    let payload: CapturedPayload = serde_json::from_slice(&plaintext)?;

    if payload.schema_version != 1 {
        warnings.push(format!(
            "payload schema {} may not be fully supported",
            payload.schema_version
        ));
    }

    for artifact in &payload.artifacts {
        if artifact.kind == ArtifactKind::DirectoryListing {
            planned.push(format!(
                "directory listing for {} ({} entries)",
                artifact.source_path.display(),
                artifact.bytes.iter().filter(|&&b| b == b'\n').count() + 1
            ));
            continue;
        }
        let dest = opts
            .target_root
            .as_ref()
            .map(|root| {
                root.join(
                    artifact
                        .source_path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| artifact.name.clone()),
                )
            })
            .unwrap_or_else(|| artifact.source_path.clone());

        if dest.exists() && !opts.overwrite {
            warnings.push(format!("skip existing {}", dest.display()));
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &artifact.bytes)?;
        applied_files += 1;
        planned.push(format!("wrote {}", dest.display()));
    }

    Ok(RestoreReport {
        dry_run: false,
        planned_actions: planned,
        warnings,
        applied_files,
    })
}
