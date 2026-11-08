use std::path::{Path, PathBuf};

use crate::artifacts::CapturedArtifact;
use crate::bundle::ImageBundle;
use crate::error::Result;
use crate::manifest::ManifestComponent;
use crate::paths::mapped_dest;
use crate::platform::{command_on_path, Platform};
use crate::profile::ComponentKind;

#[derive(Debug, Clone)]
pub struct RestoreOptions<'a> {
    pub image: &'a Path,
    pub passphrase: &'a str,
    /// Destination home used to expand `~/...`.
    pub dest_home: PathBuf,
    pub dest_platform: Platform,
    pub dry_run: bool,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreActionKind {
    Map,
    Create,
    Overwrite,
    Unchanged,
    SkipExists,
    SkipUnsupported,
    SkipMissingTool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreAction {
    pub component_id: String,
    pub kind: RestoreActionKind,
    pub detail: String,
}

impl std::fmt::Display for RestoreAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mark = match self.kind {
            RestoreActionKind::Create => "+",
            RestoreActionKind::Overwrite => "~",
            RestoreActionKind::Unchanged => "=",
            RestoreActionKind::SkipExists => "-",
            RestoreActionKind::SkipUnsupported | RestoreActionKind::SkipMissingTool => "!",
            RestoreActionKind::Map => ">",
        };
        write!(f, "{mark} [{}] {}", self.component_id, self.detail)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub dry_run: bool,
    pub source_platform: Platform,
    pub dest_platform: Platform,
    pub planned_actions: Vec<RestoreAction>,
    pub warnings: Vec<String>,
    pub applied_files: usize,
}

pub fn restore(opts: RestoreOptions<'_>) -> Result<RestoreReport> {
    let bundle = ImageBundle::open(opts.image)?;
    let inner = bundle.decrypt(opts.passphrase)?;
    let mut warnings = inner.manifest.warnings.clone();
    let mut planned = Vec::new();
    let source_platform = inner.manifest.host_platform;

    for component in &inner.manifest.components {
        plan_component(
            component,
            inner.payload.files_for(&component.id).into_iter(),
            &opts,
            &mut planned,
            &mut warnings,
        );
    }

    let mut applied_files = 0usize;
    if !opts.dry_run {
        for component in &inner.manifest.components {
            if !component.supports(opts.dest_platform) {
                continue;
            }
            let Some(dest_logical) = component.path_for(opts.dest_platform) else {
                continue;
            };
            for artifact in inner.payload.files_for(&component.id).into_iter() {
                let dest = mapped_dest(dest_logical, &artifact.relative_path, &opts.dest_home);
                applied_files += apply_file(artifact, &dest, opts.overwrite, &mut warnings)?;
            }
        }
    }

    Ok(RestoreReport {
        dry_run: opts.dry_run,
        source_platform,
        dest_platform: opts.dest_platform,
        planned_actions: planned,
        warnings,
        applied_files,
    })
}

fn plan_component<'a>(
    component: &ManifestComponent,
    files: impl Iterator<Item = &'a CapturedArtifact>,
    opts: &RestoreOptions<'_>,
    planned: &mut Vec<RestoreAction>,
    warnings: &mut Vec<String>,
) {
    if !component.supports(opts.dest_platform) {
        let supported = if component.supported_platforms.is_empty() {
            "all".into()
        } else {
            component
                .supported_platforms
                .iter()
                .map(Platform::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let hint = match component.kind {
            ComponentKind::ToolManifest if component.id == "brewfile" => {
                "Homebrew is not a restore target on this OS; capture/restore brewfile on macOS, or install Linuxbrew and enable the component."
            }
            ComponentKind::ToolManifest if component.id == "apt-packages" => {
                "apt package lists apply on Linux only."
            }
            _ => "component is not declared for this OS; nothing will be written.",
        };
        planned.push(RestoreAction {
            component_id: component.id.clone(),
            kind: RestoreActionKind::SkipUnsupported,
            detail: format!(
                "not supported on {} (supported: {supported}); {hint}",
                opts.dest_platform
            ),
        });
        warnings.push(format!(
            "component `{}` not supported on {}",
            component.id, opts.dest_platform
        ));
        return;
    }

    let Some(dest_logical) = component.path_for(opts.dest_platform) else {
        planned.push(RestoreAction {
            component_id: component.id.clone(),
            kind: RestoreActionKind::SkipUnsupported,
            detail: format!("no path mapping for {}", opts.dest_platform),
        });
        return;
    };

    if component.captured_path != dest_logical {
        planned.push(RestoreAction {
            component_id: component.id.clone(),
            kind: RestoreActionKind::Map,
            detail: format!("{} → {dest_logical}", component.captured_path),
        });
    }

    if component.kind == ComponentKind::ToolManifest {
        tool_warnings(component, opts.dest_platform, planned, warnings);
    }

    for artifact in files {
        let dest = mapped_dest(dest_logical, &artifact.relative_path, &opts.dest_home);
        let dest_s = dest.display().to_string();
        if !dest.exists() {
            planned.push(RestoreAction {
                component_id: component.id.clone(),
                kind: RestoreActionKind::Create,
                detail: format!("create {dest_s} ({} bytes)", artifact.bytes.len()),
            });
            continue;
        }
        let existing = std::fs::read(&dest).unwrap_or_default();
        if existing == artifact.bytes {
            planned.push(RestoreAction {
                component_id: component.id.clone(),
                kind: RestoreActionKind::Unchanged,
                detail: format!("unchanged {dest_s}"),
            });
        } else if opts.overwrite {
            planned.push(RestoreAction {
                component_id: component.id.clone(),
                kind: RestoreActionKind::Overwrite,
                detail: format!(
                    "overwrite {dest_s} (on disk {} bytes, image {} bytes)",
                    existing.len(),
                    artifact.bytes.len()
                ),
            });
        } else {
            planned.push(RestoreAction {
                component_id: component.id.clone(),
                kind: RestoreActionKind::SkipExists,
                detail: format!("exists {dest_s} (pass --overwrite to replace)"),
            });
            warnings.push(format!(
                "skip existing {dest_s}; pass --overwrite to replace"
            ));
        }
    }
}

fn tool_warnings(
    component: &ManifestComponent,
    dest_platform: Platform,
    planned: &mut Vec<RestoreAction>,
    warnings: &mut Vec<String>,
) {
    let (needed, hint) = match component.id.as_str() {
        "brewfile" => (
            "brew",
            "install Homebrew (macOS) or Linuxbrew, then run `brew bundle`. Cyclone only restores the Brewfile.",
        ),
        "apt-packages" => (
            "apt-get",
            "install packages from the restored list with apt; Cyclone only restores the declaration file.",
        ),
        _ => return,
    };
    let present = command_on_path(needed) || (needed == "apt-get" && command_on_path("apt"));
    if !present {
        let detail = format!("{needed} is not on PATH on {dest_platform}; {hint}");
        planned.push(RestoreAction {
            component_id: component.id.clone(),
            kind: RestoreActionKind::SkipMissingTool,
            detail: detail.clone(),
        });
        warnings.push(detail);
    }
}

fn apply_file(
    artifact: &CapturedArtifact,
    dest: &Path,
    overwrite: bool,
    warnings: &mut Vec<String>,
) -> Result<usize> {
    if dest.exists() {
        let existing = std::fs::read(dest).unwrap_or_default();
        if existing == artifact.bytes {
            return Ok(0);
        }
        if !overwrite {
            warnings.push(format!("skip existing {}", dest.display()));
            return Ok(0);
        }
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dest, &artifact.bytes)?;
    set_mode(dest, artifact.mode);
    Ok(1)
}

fn set_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if mode != 0 {
            let perms = std::fs::Permissions::from_mode(mode);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}
