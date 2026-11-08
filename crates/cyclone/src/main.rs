//! Cyclone CLI — capture, inspect, and restore encrypted developer-setup images.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use cyclone_core::{
    capture, inspect, inspect_envelope, restore, run_profiler, ApplyOptions, CaptureOptions,
    CycloneError, Distro, Extension, ImageBundle, PackageCaptureMode, PackageCaptureOptions,
    PackageManager, PackageRestoreOptions, Platform, Profile, ProfilePaths, ProfilerSession,
    ProvisionTarget, RestoreOptions, Result,
};
use cyclone_ext_exe::{ExeDevExtension, EXTENSION_NAME};
use tracing_subscriber::EnvFilter;
use zeroize::Zeroize;

#[derive(Parser)]
#[command(
    name = "cyclone",
    version,
    about = "Reusable encrypted developer-setup images",
    long_about = "Capture declared dotfiles and tooling into encrypted images; restore locally or via extensions."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// First-run profiler: scan ~/.config and ~/ , then write an editable profile.
    Init {
        /// Use discovered defaults only (CI / scripts).
        #[arg(long)]
        non_interactive: bool,
        /// Optional path to write profile (default: XDG config).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Directory used as `~` when probing for dotfiles.
        #[arg(long, env = "CYCLONE_HOME")]
        home: Option<PathBuf>,
        /// Overwrite an existing profile.
        #[arg(long)]
        force: bool,
    },
    /// View or re-run the capture profile after init (re-scan of ~/.config and ~/).
    Profile {
        /// Re-run the profiler and overwrite the saved profile.
        #[arg(long)]
        edit: bool,
        #[arg(long)]
        non_interactive: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, env = "CYCLONE_HOME")]
        home: Option<PathBuf>,
    },
    /// Snapshot declared sources into an encrypted image bundle.
    Capture {
        /// Profile TOML path (default: discovered config).
        #[arg(long)]
        profile: Option<PathBuf>,
        /// Output image directory.
        #[arg(long)]
        output: PathBuf,
        /// Passphrase for encryption (required unless --dry-run).
        #[arg(long, env = "CYCLONE_PASSPHRASE")]
        passphrase: Option<String>,
        /// Read the passphrase from a file (trim a trailing newline).
        #[arg(long)]
        key_file: Option<PathBuf>,
        #[arg(long, env = "CYCLONE_HOME")]
        home: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        /// How to capture package managers: `live` (query brew/apt/pacman/…; default), `files` (profile paths only), or `auto` (same as live).
        #[arg(long, default_value = "live", env = "CYCLONE_PACKAGE_CAPTURE")]
        packages: String,
        /// Low-memory Argon2id (tests only).
        #[arg(long, hide = true)]
        fast_kdf: bool,
    },
    /// Show manifest summary (requires the user key; secret values are never shown).
    Inspect {
        image: PathBuf,
        #[arg(long, env = "CYCLONE_PASSPHRASE")]
        passphrase: Option<String>,
        #[arg(long)]
        key_file: Option<PathBuf>,
    },
    /// Apply an image to the current machine (dry-run first).
    Restore {
        image: PathBuf,
        #[arg(long)]
        dry_run: bool,
        /// Write artifacts under this directory (acts as destination `$HOME`).
        #[arg(long)]
        target_root: Option<PathBuf>,
        /// Overwrite existing files on restore.
        #[arg(long)]
        overwrite: bool,
        #[arg(long, env = "CYCLONE_PASSPHRASE")]
        passphrase: Option<String>,
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Pretend the destination OS is linux or macos (for dry-run diffs).
        #[arg(long, value_name = "linux|macos")]
        assume_platform: Option<String>,
        /// Pretend the destination distro is ubuntu, fedora, arch, alpine, opensuse, macos, …
        #[arg(long, value_name = "DISTRO")]
        assume_distro: Option<String>,
        /// Pretend the destination package manager is homebrew, apt, dnf, pacman, yay, paru, apk, or zypper.
        #[arg(long, value_name = "MANAGER")]
        assume_manager: Option<String>,
        /// Run the matching package manager's install command (default: yes unless --target-root).
        #[arg(long)]
        install_packages: bool,
        /// Write declaration files only; do not invoke brew/apt/dnf/…
        #[arg(long)]
        no_install_packages: bool,
        #[arg(long, env = "CYCLONE_HOME")]
        home: Option<PathBuf>,
    },
    /// Upload an image to a registry (not implemented in this release).
    Push { image: PathBuf },
    /// Download an image from a registry (not implemented in this release).
    Pull { spec: String },
    /// Bootstrap a remote VM from an encrypted image (dry-run by default).
    Provision {
        image: PathBuf,
        /// Extension id. First cloud adapter: `exe-dev`.
        #[arg(long, default_value = "exe-dev")]
        ext: String,
        /// exe.dev VM name (lowercase DNS label).
        #[arg(long)]
        target: String,
        /// Print the plan only (also the default when `--apply` is omitted).
        #[arg(long)]
        dry_run: bool,
        /// Call exe.dev and restore files onto the VM.
        #[arg(long)]
        apply: bool,
        #[arg(long, env = "CYCLONE_PASSPHRASE")]
        passphrase: Option<String>,
        #[arg(long)]
        key_file: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    run(cli.command)?;
    Ok(())
}

fn run(command: Commands) -> Result<()> {
    match command {
        Commands::Init {
            non_interactive,
            output,
            home,
            force,
        } => cmd_init(non_interactive, output, home, force),
        Commands::Profile {
            edit,
            non_interactive,
            output,
            home,
        } => cmd_profile(edit, non_interactive, output, home),
        Commands::Capture {
            profile,
            output,
            passphrase,
            key_file,
            home,
            dry_run,
            packages,
            fast_kdf,
        } => cmd_capture(
            profile,
            output,
            passphrase,
            key_file,
            home,
            dry_run,
            packages,
            fast_kdf,
        ),
        Commands::Inspect {
            image,
            passphrase,
            key_file,
        } => cmd_inspect(image, passphrase, key_file),
        Commands::Restore {
            image,
            dry_run,
            target_root,
            overwrite,
            passphrase,
            key_file,
            assume_platform,
            assume_distro,
            assume_manager,
            install_packages,
            no_install_packages,
            home,
        } => cmd_restore(
            image,
            dry_run,
            target_root,
            overwrite,
            passphrase,
            key_file,
            assume_platform,
            assume_distro,
            assume_manager,
            install_packages,
            no_install_packages,
            home,
        ),
        Commands::Push { image: _ } => Err(CycloneError::User(
            "push/pull registry transport is not implemented in this release; copy the image directory (scp, USB, …) to the other machine".into(),
        )),
        Commands::Pull { spec: _ } => Err(CycloneError::User(
            "push/pull registry transport is not implemented in this release; copy the image directory (scp, USB, …) to the other machine".into(),
        )),
        Commands::Provision {
            image,
            ext,
            target,
            dry_run,
            apply,
            passphrase,
            key_file,
        } => cmd_provision(image, ext, target, dry_run, apply, passphrase, key_file),
    }
}

fn cmd_init(
    non_interactive: bool,
    output: Option<PathBuf>,
    home: Option<PathBuf>,
    force: bool,
) -> Result<()> {
    let paths = ProfilePaths::discover()?;
    let dest = output.unwrap_or(paths.profile_path);
    if Profile::exists(&dest) && !force {
        return Err(CycloneError::User(format!(
            "profile already exists at {}; use `cyclone profile` to view or `cyclone init --force` to overwrite",
            dest.display()
        )));
    }
    let home = resolve_home(home)?;
    let platform = Platform::current()?;
    let session = ProfilerSession {
        non_interactive,
        home: &home,
        platform,
        existing: None,
    };
    let profile = run_profiler(&session)?;
    profile.save(&dest)?;
    writeln!(io::stdout(), "Wrote profile to {}", dest.display())?;
    writeln!(
        io::stdout(),
        "Edit that file to change capture scope, then run `cyclone capture`."
    )?;
    Ok(())
}

fn cmd_profile(
    edit: bool,
    non_interactive: bool,
    output: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<()> {
    let paths = ProfilePaths::discover()?;
    let dest = output.unwrap_or(paths.profile_path);
    if !edit {
        if !Profile::exists(&dest) {
            return Err(CycloneError::ProfileMissing(dest.display().to_string()));
        }
        let raw = std::fs::read_to_string(&dest)?;
        writeln!(io::stdout(), "Profile: {}", dest.display())?;
        writeln!(io::stdout(), "{raw}")?;
        return Ok(());
    }
    let existing = if Profile::exists(&dest) {
        Some(Profile::load(&dest)?)
    } else {
        None
    };
    let home = resolve_home(home)?;
    let platform = Platform::current()?;
    let session = ProfilerSession {
        non_interactive,
        home: &home,
        platform,
        existing: existing.as_ref(),
    };
    let profile = run_profiler(&session)?;
    profile.save(&dest)?;
    writeln!(io::stdout(), "Wrote profile to {}", dest.display())?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_capture(
    profile_path: Option<PathBuf>,
    output: PathBuf,
    passphrase: Option<String>,
    key_file: Option<PathBuf>,
    home: Option<PathBuf>,
    dry_run: bool,
    packages: String,
    fast_kdf: bool,
) -> Result<()> {
    let paths = ProfilePaths::discover()?;
    let profile_file = profile_path.unwrap_or(paths.profile_path);
    if !Profile::exists(&profile_file) {
        return Err(CycloneError::ProfileMissing(
            profile_file.display().to_string(),
        ));
    }
    let profile = Profile::load(&profile_file)?;
    let home = resolve_home(home)?;
    let mut phrase = if dry_run {
        String::new()
    } else {
        resolve_passphrase(passphrase, key_file, true)?
    };
    let report = capture(CaptureOptions {
        profile: &profile,
        output: &output,
        passphrase: if dry_run { None } else { Some(phrase.as_str()) },
        home,
        platform: Platform::current()?,
        dry_run,
        fast_kdf,
        packages: PackageCaptureOptions {
            mode: packages.parse::<PackageCaptureMode>()?,
            distro: None,
            commands: Default::default(),
        },
    })?;
    phrase.zeroize();
    writeln!(
        io::stdout(),
        "capture {}: {} components",
        if report.dry_run { "dry-run" } else { "ok" },
        report.component_count
    )?;
    for action in report.actions {
        writeln!(io::stdout(), "  {action}")?;
    }
    for w in report.warnings {
        writeln!(io::stdout(), "warning: {w}")?;
    }
    Ok(())
}

fn cmd_inspect(
    image: PathBuf,
    passphrase: Option<String>,
    key_file: Option<PathBuf>,
) -> Result<()> {
    let mut phrase = match resolve_passphrase(passphrase, key_file, false) {
        Ok(p) if !p.is_empty() => p,
        _ => {
            let (aead, kdf) = inspect_envelope(&image)?;
            return Err(CycloneError::Crypto(format!(
                "passphrase required to inspect encrypted image (use --passphrase or CYCLONE_PASSPHRASE); envelope is {kdf} + {aead}"
            )));
        }
    };
    let report = inspect(&image, &phrase)?;
    phrase.zeroize();
    writeln!(io::stdout(), "image: {}", report.path)?;
    writeln!(
        io::stdout(),
        "captured: {} on {}/{}",
        report.captured_at,
        report.host_platform,
        report.host_arch
    )?;
    if let Some(distro) = &report.host_distro {
        write!(io::stdout(), "distro: {distro}")?;
        if let Some(pm) = &report.host_package_manager {
            write!(io::stdout(), " ({pm})")?;
        }
        writeln!(io::stdout())?;
    }
    writeln!(io::stdout(), "encryption: {} + {}", report.kdf, report.aead)?;
    writeln!(io::stdout(), "components: {}", report.component_count)?;
    for c in report.components {
        writeln!(
            io::stdout(),
            "  - {} ({}, {}, {} files, {} bytes, sha256={})",
            c.id,
            c.kind.as_str(),
            c.captured_path,
            c.file_count,
            c.byte_size,
            &c.content_sha256[..c.content_sha256.len().min(12)]
        )?;
    }
    if !report.secret_names.is_empty() {
        writeln!(
            io::stdout(),
            "secrets (names only): {}",
            report.secret_names.join(", ")
        )?;
    }
    for w in report.warnings {
        writeln!(io::stdout(), "warning: {w}")?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_restore(
    image: PathBuf,
    dry_run: bool,
    target_root: Option<PathBuf>,
    overwrite: bool,
    passphrase: Option<String>,
    key_file: Option<PathBuf>,
    assume_platform: Option<String>,
    assume_distro: Option<String>,
    assume_manager: Option<String>,
    install_packages: bool,
    no_install_packages: bool,
    home: Option<PathBuf>,
) -> Result<()> {
    if install_packages && no_install_packages {
        return Err(CycloneError::User(
            "pass --install-packages or --no-install-packages, not both".into(),
        ));
    }
    let mut phrase = resolve_passphrase(passphrase, key_file, true)?;
    let dest_platform = match assume_platform {
        Some(s) => s.parse()?,
        None => Platform::current()?,
    };
    let using_target_root = target_root.is_some();
    let dest_home = match target_root {
        Some(root) => root,
        None => resolve_home(home)?,
    };
    let install = if no_install_packages {
        false
    } else if install_packages {
        true
    } else {
        !dry_run && !using_target_root
    };
    let report = restore(RestoreOptions {
        image: &image,
        passphrase: &phrase,
        dest_home,
        dest_platform,
        dry_run,
        overwrite,
        packages: PackageRestoreOptions {
            distro: assume_distro.as_deref().map(Distro::from_id),
            manager: assume_manager
                .as_deref()
                .map(|s| s.parse::<PackageManager>())
                .transpose()?,
            install,
            commands: Default::default(),
        },
    })?;
    phrase.zeroize();
    writeln!(
        io::stdout(),
        "restore {} ({} → {}, {} actions, {} files)",
        if report.dry_run { "dry-run" } else { "ok" },
        report.source_platform,
        report.dest_platform,
        report.planned_actions.len(),
        report.applied_files
    )?;
    for action in report.planned_actions {
        writeln!(io::stdout(), "  {action}")?;
    }
    for w in report.warnings {
        writeln!(io::stdout(), "warning: {w}")?;
    }
    Ok(())
}

fn cmd_provision(
    image: PathBuf,
    ext: String,
    target: String,
    dry_run_flag: bool,
    apply_flag: bool,
    passphrase: Option<String>,
    key_file: Option<PathBuf>,
) -> Result<()> {
    if dry_run_flag && apply_flag {
        return Err(CycloneError::User(
            "pass --dry-run or --apply, not both".into(),
        ));
    }
    let dry_run = !apply_flag;
    if ext != EXTENSION_NAME {
        return Err(CycloneError::User(format!(
            "unknown extension `{ext}` (supported: {EXTENSION_NAME})"
        )));
    }
    let mut phrase = resolve_passphrase(passphrase, key_file, true)?;
    let bundle = ImageBundle::open(&image)?;
    writeln!(
        io::stdout(),
        "encrypted image: {} + {}",
        bundle.header.kdf.algorithm,
        bundle.header.aead
    )?;
    let inner = bundle.decrypt(&phrase)?;
    phrase.zeroize();
    let extension = if dry_run {
        ExeDevExtension::new()
    } else {
        ExeDevExtension::from_env()?
    };
    let target = ProvisionTarget {
        id: target,
        labels: vec![],
    };
    let plan = extension.plan(&inner, &target)?;
    writeln!(
        io::stdout(),
        "provision plan ({EXTENSION_NAME} → VM `{}`, {} steps):",
        plan.target.id,
        plan.steps.len()
    )?;
    for step in &plan.steps {
        writeln!(io::stdout(), "  - {}: {}", step.action, step.detail)?;
    }
    let result = extension.apply(&inner, &target, &ApplyOptions { dry_run })?;
    writeln!(
        io::stdout(),
        "provision {}",
        if result.dry_run { "dry-run" } else { "ok" }
    )?;
    for line in result.log {
        writeln!(io::stdout(), "  {line}")?;
    }
    Ok(())
}

fn resolve_home(home: Option<PathBuf>) -> Result<PathBuf> {
    match home {
        Some(h) => Ok(h),
        None => cyclone_core::default_home(),
    }
}

fn resolve_passphrase(
    passphrase: Option<String>,
    key_file: Option<PathBuf>,
    required: bool,
) -> Result<String> {
    if passphrase.is_some() && key_file.is_some() {
        return Err(CycloneError::User(
            "pass --passphrase or --key-file, not both".into(),
        ));
    }
    if let Some(path) = key_file {
        let mut raw = std::fs::read_to_string(&path)?;
        let trimmed = raw.trim_end_matches(['\n', '\r']).to_string();
        raw.zeroize();
        if trimmed.is_empty() {
            return Err(CycloneError::Crypto("key file is empty".into()));
        }
        return Ok(trimmed);
    }
    match passphrase {
        Some(p) if !p.is_empty() => Ok(p),
        _ if required => Err(CycloneError::Crypto(
            "passphrase required (use --passphrase, --key-file, or CYCLONE_PASSPHRASE)".into(),
        )),
        _ => Ok(String::new()),
    }
}
