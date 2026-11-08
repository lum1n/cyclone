use std::path::Path;

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::artifacts::{ArtifactKind, CapturedArtifact, CapturedPayload};
use crate::bundle::ImageBundle;
use crate::crypto::EncryptionKey;
use crate::error::{CycloneError, Result};
use crate::manifest::{utc_now_rfc3339, ImageManifest, ManifestComponent};
use crate::platform::Platform;
use crate::profile::Profile;

/// Maximum number of directory entries recorded in a single capture root (safety cap).
const MAX_DIR_ENTRIES: usize = 10_000;

#[derive(Debug, Clone)]
pub struct CaptureOptions<'a> {
    pub profile: &'a Profile,
    pub output: &'a Path,
    pub encryption_key: &'a EncryptionKey,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureReport {
    pub dry_run: bool,
    pub output: String,
    pub component_count: usize,
    pub warnings: Vec<String>,
}

pub fn capture(opts: CaptureOptions<'_>) -> Result<CaptureReport> {
    opts.profile.validate()?;
    let host_platform = Platform::current()?;
    let mut warnings = Vec::new();
    let mut components = Vec::new();
    let mut artifacts = Vec::new();

    for root in &opts.profile.capture_roots {
        if !root.exists() {
            warnings.push(format!("skipped missing path: {}", root.display()));
            continue;
        }
        if root.is_file() {
            let content = std::fs::read(root)?;
            let hash = hex::encode(Sha256::digest(&content));
            let name = component_name(root);
            components.push(ManifestComponent {
                name: name.clone(),
                kind: "file".into(),
                source_path: root.display().to_string(),
                content_sha256: hash,
                supported_platforms: vec![host_platform],
            });
            artifacts.push(CapturedArtifact {
                name,
                source_path: root.clone(),
                kind: ArtifactKind::File,
                bytes: content,
            });
        } else if root.is_dir() {
            let (listing, files, dir_warnings) = capture_directory(root)?;
            warnings.extend(dir_warnings);
            let listing_bytes = listing.join("\n").into_bytes();
            let hash = hex::encode(Sha256::digest(&listing_bytes));
            let name = component_name(root);
            components.push(ManifestComponent {
                name: name.clone(),
                kind: "directory".into(),
                source_path: root.display().to_string(),
                content_sha256: hash,
                supported_platforms: vec![host_platform],
            });
            artifacts.push(CapturedArtifact {
                name: name.clone(),
                source_path: root.clone(),
                kind: ArtifactKind::DirectoryListing,
                bytes: listing_bytes,
            });
            artifacts.extend(files);
        }
    }

    for tool in &opts.profile.tool_declarations {
        if !tool.supported_platforms.contains(&host_platform) {
            warnings.push(format!(
                "tool `{}` not declared for {}",
                tool.name,
                host_platform.as_str()
            ));
        }
    }

    let manifest = ImageManifest {
        schema_version: 1,
        captured_at: utc_now_rfc3339(),
        host_platform,
        host_arch: std::env::consts::ARCH.to_string(),
        components: components.clone(),
        secret_references: opts.profile.secret_references.clone(),
        warnings: warnings.clone(),
    };

    if opts.dry_run {
        return Ok(CaptureReport {
            dry_run: true,
            output: opts.output.display().to_string(),
            component_count: components.len(),
            warnings,
        });
    }

    if opts.output.exists() {
        return Err(CycloneError::Other(anyhow::anyhow!(
            "output already exists: {}",
            opts.output.display()
        )));
    }

    let bundle = ImageBundle::create_at(opts.output, manifest)?;
    let payload = CapturedPayload::new(artifacts);
    let plaintext = serde_json::to_vec(&payload)?;
    bundle.write_encrypted_payload(&plaintext, opts.encryption_key)?;

    Ok(CaptureReport {
        dry_run: false,
        output: opts.output.display().to_string(),
        component_count: components.len(),
        warnings,
    })
}

fn component_name(root: &Path) -> String {
    root.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}

fn capture_directory(root: &Path) -> Result<(Vec<String>, Vec<CapturedArtifact>, Vec<String>)> {
    let mut listing = Vec::new();
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let mut count = 0usize;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path() != root)
    {
        count += 1;
        if count > MAX_DIR_ENTRIES {
            warnings.push(format!(
                "truncated directory walk at {} entries for {}",
                MAX_DIR_ENTRIES,
                root.display()
            ));
            break;
        }
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        if entry.file_type().is_dir() {
            listing.push(format!("{rel}/"));
            continue;
        }
        if entry.file_type().is_file() {
            listing.push(rel.clone());
            let content = std::fs::read(path)?;
            files.push(CapturedArtifact {
                name: rel.replace('/', "__"),
                source_path: path.to_path_buf(),
                kind: ArtifactKind::File,
                bytes: content,
            });
        }
    }

    Ok((listing, files, warnings))
}
