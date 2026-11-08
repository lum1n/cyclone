use std::path::{Path, PathBuf};

use crate::artifacts::CapturedArtifact;
use crate::bundle::{ImageBundle, InnerBundle};
use crate::error::Result;
use crate::manifest::ManifestComponent;
use crate::packages::{
    infer_package_manager, install_command_line, is_arch_family, mismatch_detail,
    missing_aur_helper_detail, parse_declaration, run_install, summarize_names, HostEnv,
    PackageDeclaration, PackageRestoreOptions,
};
use crate::paths::mapped_dest;
use crate::platform::Platform;
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
    pub packages: PackageRestoreOptions,
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
    SkipManagerMismatch,
    WouldInstall,
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
            RestoreActionKind::SkipUnsupported
            | RestoreActionKind::SkipMissingTool
            | RestoreActionKind::SkipManagerMismatch => "!",
            RestoreActionKind::WouldInstall => "+",
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
    let dest_host = HostEnv::assumed(
        opts.dest_platform,
        opts.packages.distro.clone(),
        opts.packages.manager,
        &opts.packages.commands,
    );

    for component in &inner.manifest.components {
        plan_component(
            component,
            inner.payload.files_for(&component.id).into_iter(),
            &opts,
            &dest_host,
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
            if opts.packages.install {
                if let Err(e) = apply_packages(component, &inner, &opts, &dest_host, dest_logical) {
                    warnings.push(format!(
                        "package install for `{}` failed: {e}",
                        component.id
                    ));
                }
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
    dest_host: &HostEnv,
    planned: &mut Vec<RestoreAction>,
    warnings: &mut Vec<String>,
) {
    let files: Vec<&'a CapturedArtifact> = files.collect();
    let decl = declaration_from_files(component, &files);

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
        let hint = unsupported_hint(component, decl.as_ref(), dest_host);
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

    for artifact in &files {
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

    if let Some(decl) = decl {
        plan_package_restore(
            component,
            &decl,
            dest_logical,
            opts,
            dest_host,
            planned,
            warnings,
        );
    }
}

fn declaration_from_files(
    component: &ManifestComponent,
    files: &[&CapturedArtifact],
) -> Option<PackageDeclaration> {
    let manager = infer_package_manager(
        &component.id,
        component.linux.as_deref(),
        component.macos.as_deref(),
        component.package_manager,
    )?;
    let artifact = files.first()?;
    let body = String::from_utf8_lossy(&artifact.bytes);
    Some(parse_declaration(manager, "", &body))
}

fn unsupported_hint(
    component: &ManifestComponent,
    decl: Option<&PackageDeclaration>,
    dest_host: &HostEnv,
) -> String {
    if let Some(decl) = decl {
        return mismatch_detail(decl, dest_host);
    }
    match component.kind {
        ComponentKind::ToolManifest if component.id == "brewfile" => {
            "Homebrew declarations apply on a host with brew (typically macOS).".into()
        }
        ComponentKind::ToolManifest if component.id == "apt-packages" => {
            "apt package lists apply on Debian/Ubuntu (apt), not this OS.".into()
        }
        _ => "component is not declared for this OS; nothing will be written.".into(),
    }
}

fn plan_package_restore(
    component: &ManifestComponent,
    decl: &PackageDeclaration,
    dest_logical: &str,
    opts: &RestoreOptions<'_>,
    dest_host: &HostEnv,
    planned: &mut Vec<RestoreAction>,
    warnings: &mut Vec<String>,
) {
    if decl.manager.is_aur_helper() {
        if !is_arch_family(&dest_host.distro) {
            let detail = mismatch_detail(decl, dest_host);
            planned.push(RestoreAction {
                component_id: component.id.clone(),
                kind: RestoreActionKind::SkipManagerMismatch,
                detail: detail.clone(),
            });
            warnings.push(detail);
            return;
        }
        if !decl.manager.present_on(&opts.packages.commands) {
            let detail = missing_aur_helper_detail(decl, dest_host);
            planned.push(RestoreAction {
                component_id: component.id.clone(),
                kind: RestoreActionKind::SkipMissingTool,
                detail: detail.clone(),
            });
            warnings.push(detail);
            return;
        }
    } else {
        if dest_host.manager != Some(decl.manager) {
            let detail = mismatch_detail(decl, dest_host);
            planned.push(RestoreAction {
                component_id: component.id.clone(),
                kind: RestoreActionKind::SkipManagerMismatch,
                detail: detail.clone(),
            });
            warnings.push(detail);
            return;
        }
        if !decl.manager.present_on(&opts.packages.commands) {
            let needed = decl.manager.command();
            let detail = format!(
                "{needed} is not on PATH on {} ({}); install {} then re-run restore. packages: {}",
                dest_host.platform,
                dest_host.distro.id,
                decl.manager.display_name(),
                summarize_names(&decl.packages)
            );
            planned.push(RestoreAction {
                component_id: component.id.clone(),
                kind: RestoreActionKind::SkipMissingTool,
                detail: detail.clone(),
            });
            warnings.push(detail);
            return;
        }
    }
    let dest_file = mapped_dest(dest_logical, "", &opts.dest_home);
    let use_sudo = decl.manager.install_needs_root();
    let cmd = install_command_line(decl.manager, &decl.packages, Some(&dest_file), use_sudo);
    planned.push(RestoreAction {
        component_id: component.id.clone(),
        kind: RestoreActionKind::WouldInstall,
        detail: format!(
            "would run `{cmd}` ({} packages: {})",
            decl.packages.len(),
            summarize_names(&decl.packages)
        ),
    });
}

fn apply_packages(
    component: &ManifestComponent,
    inner: &InnerBundle,
    opts: &RestoreOptions<'_>,
    dest_host: &HostEnv,
    dest_logical: &str,
) -> Result<()> {
    let files = inner.payload.files_for(&component.id);
    let Some(decl) = declaration_from_files(component, &files) else {
        return Ok(());
    };
    if decl.manager.is_aur_helper() {
        if !is_arch_family(&dest_host.distro) || !decl.manager.present_on(&opts.packages.commands) {
            return Ok(());
        }
    } else if dest_host.manager != Some(decl.manager)
        || !decl.manager.present_on(&opts.packages.commands)
    {
        return Ok(());
    }
    let dest_file = mapped_dest(dest_logical, "", &opts.dest_home);
    run_install(
        decl.manager,
        &decl.packages,
        Some(&dest_file),
        &opts.packages.commands,
    )
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
