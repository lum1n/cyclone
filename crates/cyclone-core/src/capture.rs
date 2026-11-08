use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::artifacts::{ArtifactKind, CapturedArtifact, CapturedPayload};
use crate::bundle::{ImageBundle, InnerBundle};
use crate::error::Result;
use crate::manifest::{utc_now_rfc3339, ImageManifest, ManifestComponent};
use crate::packages::{
    dump_live, parse_declaration, should_live_dump, should_read_file, HostEnv,
    PackageCaptureOptions, PackageDeclaration, PackageManager,
};
use crate::paths::{expand_tilde, normalize_rel};
use crate::platform::Platform;
use crate::profile::{ComponentKind, Profile, ProfileComponent};

/// Maximum number of directory entries recorded in a single capture root (safety cap).
const MAX_DIR_ENTRIES: usize = 10_000;
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
const SKIP_DIR_NAMES: &[&str] = &["node_modules", ".cache", "Cache", "GPUCache"];

#[derive(Debug, Clone)]
pub struct CaptureOptions<'a> {
    pub profile: &'a Profile,
    pub output: &'a Path,
    pub passphrase: Option<&'a str>,
    pub home: PathBuf,
    pub platform: Platform,
    pub dry_run: bool,
    pub fast_kdf: bool,
    pub packages: PackageCaptureOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureReport {
    pub dry_run: bool,
    pub output: String,
    pub component_count: usize,
    pub actions: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn capture(opts: CaptureOptions<'_>) -> Result<CaptureReport> {
    opts.profile.validate()?;
    let host = HostEnv::detect_for(opts.platform, &opts.packages.commands)?;
    let host = HostEnv {
        distro: opts.packages.distro.clone().unwrap_or(host.distro),
        ..host
    };
    let mut warnings = Vec::new();
    let mut actions = Vec::new();
    let mut components = Vec::new();
    let mut artifacts = Vec::new();

    for component in opts.profile.included() {
        if !component.supports(opts.platform) {
            warnings.push(format!(
                "skip `{}`: not declared for {} (supported: {})",
                component.id,
                opts.platform,
                platforms_csv(&component.supported_platforms)
            ));
            continue;
        }
        let Some(logical) = component.path_for(opts.platform) else {
            warnings.push(format!(
                "skip `{}`: no path mapping for {}",
                component.id, opts.platform
            ));
            continue;
        };
        let abs = expand_tilde(logical, &opts.home);
        let pm = component
            .package_manager
            .or_else(|| PackageManager::from_component_id(&component.id));

        if let Some(pm) = pm {
            if component.kind != ComponentKind::Dotfile {
                match capture_packages(component, logical, &abs, pm, &host, &opts, &mut warnings) {
                    Ok(Some(decl)) => {
                        push_declaration(
                            component,
                            logical,
                            decl,
                            &mut components,
                            &mut artifacts,
                            &mut actions,
                        );
                        continue;
                    }
                    Ok(None) => continue,
                    Err(e) => {
                        warnings.push(format!("skip `{}`: package dump failed: {e}", component.id));
                        continue;
                    }
                }
            }
        }

        if !abs.exists() {
            warnings.push(format!(
                "skip `{}`: missing path {} ({logical})",
                component.id,
                abs.display()
            ));
            continue;
        }

        match component.kind {
            ComponentKind::Dotfile | ComponentKind::ToolManifest => {
                if abs.is_dir() {
                    warnings.push(format!(
                        "skip `{}`: expected a file at {}",
                        component.id,
                        abs.display()
                    ));
                    continue;
                }
                match read_file(&abs) {
                    FileRead::Skipped(reason) => {
                        warnings.push(format!("skip `{}`: {reason}", component.id))
                    }
                    FileRead::Ok { bytes, mode, hash } => {
                        let byte_size = bytes.len() as u64;
                        components.push(manifest_file(component, logical, hash, byte_size, pm));
                        artifacts.push(CapturedArtifact {
                            component_id: component.id.clone(),
                            relative_path: String::new(),
                            kind: ArtifactKind::File,
                            bytes,
                            mode,
                        });
                        actions.push(format!(
                            "+ capture [{}] {logical} ({byte_size} bytes)",
                            component.id
                        ));
                    }
                }
            }
            ComponentKind::ConfigTree => {
                if !abs.is_dir() {
                    warnings.push(format!(
                        "skip `{}`: expected a directory at {}",
                        component.id,
                        abs.display()
                    ));
                    continue;
                }
                let (files, dir_warnings) = capture_directory(component, &abs)?;
                warnings.extend(dir_warnings);
                if files.is_empty() {
                    warnings.push(format!(
                        "skip `{}`: no files captured under {}",
                        component.id,
                        abs.display()
                    ));
                    continue;
                }
                let byte_size: u64 = files.iter().map(|f| f.bytes.len() as u64).sum();
                let hash = tree_hash(&files);
                let count = files.len();
                components.push(ManifestComponent {
                    id: component.id.clone(),
                    kind: component.kind,
                    linux: component.linux.clone(),
                    macos: component.macos.clone(),
                    captured_path: logical.to_string(),
                    content_sha256: hash,
                    file_count: count,
                    byte_size,
                    supported_platforms: component.supported_platforms.clone(),
                    package_manager: pm,
                });
                actions.push(format!(
                    "+ capture [{}] {logical} ({count} files, {byte_size} bytes)",
                    component.id
                ));
                artifacts.extend(files);
            }
        }
    }

    if opts.dry_run {
        return Ok(CaptureReport {
            dry_run: true,
            output: opts.output.display().to_string(),
            component_count: components.len(),
            actions,
            warnings,
        });
    }

    let passphrase = opts.passphrase.ok_or_else(|| {
        crate::error::CycloneError::Crypto(
            "passphrase required to encrypt an image (use --passphrase or CYCLONE_PASSPHRASE)"
                .into(),
        )
    })?;

    let manifest = ImageManifest {
        schema_version: 1,
        captured_at: utc_now_rfc3339(),
        host_platform: opts.platform,
        host_arch: std::env::consts::ARCH.to_string(),
        host_distro: Some(host.distro.id.clone()),
        host_package_manager: host.manager,
        components: components.clone(),
        secret_references: opts.profile.secret_references.clone(),
        warnings: warnings.clone(),
    };
    let inner = InnerBundle {
        manifest,
        payload: CapturedPayload::new(artifacts),
    };
    ImageBundle::write(opts.output, &inner, passphrase, opts.fast_kdf)?;

    Ok(CaptureReport {
        dry_run: false,
        output: opts.output.display().to_string(),
        component_count: components.len(),
        actions,
        warnings,
    })
}

fn capture_packages(
    component: &ProfileComponent,
    logical: &str,
    abs: &Path,
    manager: PackageManager,
    host: &HostEnv,
    opts: &CaptureOptions<'_>,
    warnings: &mut Vec<String>,
) -> Result<Option<PackageDeclaration>> {
    let file_exists = abs.is_file();
    if should_read_file(opts.packages.mode, file_exists) {
        return read_declaration_file(component, abs, manager, &host.distro.id, warnings);
    }
    if should_live_dump(opts.packages.mode) {
        if manager.present_on(&opts.packages.commands) {
            match dump_live(manager, &host.distro, &opts.packages.commands)? {
                Some(decl) => return Ok(Some(decl)),
                None => {
                    warnings.push(missing_manager_warning(component, manager, host));
                    return Ok(None);
                }
            }
        }
        // Homebrew only: a user-opted-in Brewfile is captured as a file when `brew` is absent.
        if manager == PackageManager::Homebrew && file_exists {
            return read_declaration_file(component, abs, manager, &host.distro.id, warnings);
        }
        warnings.push(missing_manager_warning(component, manager, host));
        return Ok(None);
    }
    warnings.push(format!(
        "skip `{}`: --packages files requires a declaration at {logical} \
(default capture queries `{}` instead; no user-maintained list file is needed)",
        component.id,
        manager.command()
    ));
    Ok(None)
}

fn read_declaration_file(
    component: &ProfileComponent,
    abs: &Path,
    manager: PackageManager,
    distro_id: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<PackageDeclaration>> {
    match read_file(abs) {
        FileRead::Ok { bytes, .. } => {
            let body = String::from_utf8_lossy(&bytes);
            Ok(Some(parse_declaration(manager, distro_id, &body)))
        }
        FileRead::Skipped(reason) => {
            warnings.push(format!("skip `{}`: {reason}", component.id));
            Ok(None)
        }
    }
}

fn missing_manager_warning(
    component: &ProfileComponent,
    manager: PackageManager,
    host: &HostEnv,
) -> String {
    format!(
        "skip `{}`: `{}` is not on PATH on {} ({}); \
Cyclone lists installed packages by running the manager (no user-maintained list file is required)",
        component.id,
        manager.command(),
        host.platform,
        host.distro.id
    )
}

fn push_declaration(
    component: &ProfileComponent,
    logical: &str,
    decl: PackageDeclaration,
    components: &mut Vec<ManifestComponent>,
    artifacts: &mut Vec<CapturedArtifact>,
    actions: &mut Vec<String>,
) {
    let bytes = decl.body.into_bytes();
    let byte_size = bytes.len() as u64;
    let hash = hex::encode(Sha256::digest(&bytes));
    let count = decl.packages.len();
    components.push(manifest_file(
        component,
        logical,
        hash,
        byte_size,
        Some(decl.manager),
    ));
    artifacts.push(CapturedArtifact {
        component_id: component.id.clone(),
        relative_path: String::new(),
        kind: ArtifactKind::File,
        bytes,
        mode: 0o644,
    });
    actions.push(format!(
        "+ capture [{}] {} {} declaration ({count} packages, {byte_size} bytes)",
        component.id,
        decl.manager.display_name(),
        logical
    ));
}

fn manifest_file(
    component: &ProfileComponent,
    logical: &str,
    hash: String,
    byte_size: u64,
    package_manager: Option<PackageManager>,
) -> ManifestComponent {
    ManifestComponent {
        id: component.id.clone(),
        kind: component.kind,
        linux: component.linux.clone(),
        macos: component.macos.clone(),
        captured_path: logical.to_string(),
        content_sha256: hash,
        file_count: 1,
        byte_size,
        supported_platforms: component.supported_platforms.clone(),
        package_manager,
    }
}

enum FileRead {
    Ok {
        bytes: Vec<u8>,
        mode: u32,
        hash: String,
    },
    Skipped(String),
}

fn read_file(path: &Path) -> FileRead {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return FileRead::Skipped(e.to_string()),
    };
    if meta.len() > MAX_FILE_BYTES {
        return FileRead::Skipped(format!(
            "file larger than {MAX_FILE_BYTES} bytes ({})",
            path.display()
        ));
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return FileRead::Skipped(e.to_string()),
    };
    let hash = hex::encode(Sha256::digest(&bytes));
    FileRead::Ok {
        bytes,
        mode: unix_mode(path),
        hash,
    }
}

fn capture_directory(
    component: &ProfileComponent,
    root: &Path,
) -> Result<(Vec<CapturedArtifact>, Vec<String>)> {
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let mut count = 0usize;

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("walk error under {}: {e}", root.display()));
                continue;
            }
        };
        if entry.path() == root {
            continue;
        }
        if entry.file_type().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if SKIP_DIR_NAMES.contains(&name) {
                    warnings.push(format!(
                        "skip directory {} (denylist)",
                        entry.path().display()
                    ));
                }
            }
            continue;
        }
        if entry.path_is_symlink() || entry.file_type().is_symlink() {
            warnings.push(format!("skip symlink {}", entry.path().display()));
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        if entry
            .path()
            .components()
            .any(|c| SKIP_DIR_NAMES.contains(&c.as_os_str().to_string_lossy().as_ref()))
        {
            continue;
        }
        count += 1;
        if count > MAX_DIR_ENTRIES {
            warnings.push(format!(
                "truncated directory walk at {MAX_DIR_ENTRIES} entries for {}",
                root.display()
            ));
            break;
        }
        let rel = match entry.path().strip_prefix(root) {
            Ok(r) => normalize_rel(r),
            Err(_) => continue,
        };
        match read_file(entry.path()) {
            FileRead::Skipped(reason) => warnings.push(reason),
            FileRead::Ok {
                bytes,
                mode,
                hash: _,
            } => {
                files.push(CapturedArtifact {
                    component_id: component.id.clone(),
                    relative_path: rel,
                    kind: ArtifactKind::File,
                    bytes,
                    mode,
                });
            }
        }
    }

    Ok((files, warnings))
}

fn tree_hash(files: &[CapturedArtifact]) -> String {
    let mut items: Vec<(&str, [u8; 32])> = files
        .iter()
        .map(|f| {
            let digest = Sha256::digest(&f.bytes);
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&digest);
            (f.relative_path.as_str(), hash)
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(b.0));
    let mut hasher = Sha256::new();
    for (rel, hash) in items {
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        hasher.update(hash);
    }
    hex::encode(hasher.finalize())
}

fn unix_mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode())
            .unwrap_or(0o644)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0o644
    }
}

fn platforms_csv(platforms: &[Platform]) -> String {
    if platforms.is_empty() {
        "all".into()
    } else {
        platforms
            .iter()
            .map(Platform::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }
}
