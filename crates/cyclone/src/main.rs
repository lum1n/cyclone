//! Cyclone CLI — capture, inspect, and restore encrypted developer-setup images.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use cyclone_core::{
    capture, inspect, inspect_envelope, restore, run_profiler, CaptureOptions, CycloneError,
    Extension, ImageBundle, Platform, Profile, ProfilePaths, ProfilerSession, ProvisionTarget,
    RestoreOptions, Result,
};
use cyclone_ext_exe::ExeDevExtension;
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
    /// First-run profiler: discover capture candidates and write an editable profile.
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
    /// View or re-run the capture profile after init.
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
        #[arg(long, env = "CYCLONE_HOME")]
        home: Option<PathBuf>,
    },
    /// Upload an image to a registry (not implemented in this release).
    Push { image: PathBuf },
    /// Download an image from a registry (not implemented in this release).
    Pull { spec: String },
    /// Provision a remote target via an extension (exe.dev is a stub).
    Provision {
        image: PathBuf,
        #[arg(long, default_value = "exe")]
        ext: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        dry_run: bool,
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
            fast_kdf,
        } => cmd_capture(profile, output, passphrase, key_file, home, dry_run, fast_kdf),
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
            home,
        } => cmd_restore(
            image,
            dry_run,
            target_root,
            overwrite,
            passphrase,
            key_file,
            assume_platform,
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
            passphrase,
            key_file,
        } => cmd_provision(image, ext, target, dry_run, passphrase, key_file),
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

fn cmd_capture(
    profile_path: Option<PathBuf>,
    output: PathBuf,
    passphrase: Option<String>,
    key_file: Option<PathBuf>,
    home: Option<PathBuf>,
    dry_run: bool,
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
    home: Option<PathBuf>,
) -> Result<()> {
    let mut phrase = resolve_passphrase(passphrase, key_file, true)?;
    let dest_platform = match assume_platform {
        Some(s) => s.parse()?,
        None => Platform::current()?,
    };
    let dest_home = match target_root {
        Some(root) => root,
        None => resolve_home(home)?,
    };
    let report = restore(RestoreOptions {
        image: &image,
        passphrase: &phrase,
        dest_home,
        dest_platform,
        dry_run,
        overwrite,
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
    dry_run: bool,
    passphrase: Option<String>,
    key_file: Option<PathBuf>,
) -> Result<()> {
    if ext != "exe" {
        return Err(CycloneError::User(format!(
            "unknown extension `{ext}` (supported: exe)"
        )));
    }
    let mut phrase = resolve_passphrase(passphrase, key_file, true)?;
    let bundle = ImageBundle::open(&image)?;
    let inner = bundle.decrypt(&phrase)?;
    phrase.zeroize();
    let extension = ExeDevExtension::new();
    let plan = extension.plan(
        &inner.manifest,
        &ProvisionTarget {
            id: target,
            labels: vec![],
        },
    )?;
    let log = extension.apply(&plan, dry_run)?;
    for line in log {
        writeln!(io::stdout(), "{line}")?;
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
