//! Cyclone CLI — capture, inspect, and restore encrypted developer-setup images.

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use cyclone_core::{
    capture, inspect, restore, run_interactive_profiler, CaptureOptions, EncryptionKey, Extension,
    ImageBundle, Profile, ProfilePaths, ProfilerSession, ProvisionTarget, RestoreOptions, Result,
};
use cyclone_ext_exe::ExeDevExtension;
use tracing_subscriber::EnvFilter;

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
    /// Create or refresh the local capture profile (interactive on first run).
    Profile {
        /// Use defaults only (CI / scripts).
        #[arg(long)]
        non_interactive: bool,
        /// Optional path to write profile (default: XDG config).
        #[arg(long)]
        output: Option<PathBuf>,
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
        #[arg(long)]
        dry_run: bool,
    },
    /// Show manifest summary (secrets redacted).
    Inspect { image: PathBuf },
    /// Apply an image to the current machine (dry-run supported).
    Restore {
        image: PathBuf,
        #[arg(long)]
        dry_run: bool,
        /// Write file artifacts under this directory instead of original paths.
        #[arg(long)]
        target_root: Option<PathBuf>,
        /// Overwrite existing files on restore.
        #[arg(long)]
        overwrite: bool,
        #[arg(long, env = "CYCLONE_PASSPHRASE")]
        passphrase: Option<String>,
    },
    /// Provision a remote target via an extension.
    Provision {
        image: PathBuf,
        #[arg(long, default_value = "exe")]
        ext: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        dry_run: bool,
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
        Commands::Profile {
            non_interactive,
            output,
        } => cmd_profile(non_interactive, output),
        Commands::Capture {
            profile,
            output,
            passphrase,
            dry_run,
        } => cmd_capture(profile, output, passphrase, dry_run),
        Commands::Inspect { image } => cmd_inspect(image),
        Commands::Restore {
            image,
            dry_run,
            target_root,
            overwrite,
            passphrase,
        } => cmd_restore(image, dry_run, target_root, overwrite, passphrase),
        Commands::Provision {
            image,
            ext,
            target,
            dry_run,
        } => cmd_provision(image, ext, target, dry_run),
    }
}

fn cmd_profile(non_interactive: bool, output: Option<PathBuf>) -> Result<()> {
    let paths = ProfilePaths::discover()?;
    let dest = output.unwrap_or(paths.profile_path);
    let session = ProfilerSession {
        non_interactive: non_interactive || !io::stdout().is_terminal(),
    };
    let profile = run_interactive_profiler(&session)?;
    profile.save(&dest)?;
    writeln!(io::stdout(), "Wrote profile to {}", dest.display())?;
    Ok(())
}

fn cmd_capture(
    profile_path: Option<PathBuf>,
    output: PathBuf,
    passphrase: Option<String>,
    dry_run: bool,
) -> Result<()> {
    let paths = ProfilePaths::discover()?;
    let profile_file = profile_path.unwrap_or(paths.profile_path);
    if !Profile::exists(&profile_file) {
        writeln!(
            io::stderr(),
            "No profile at {}; running profiler first",
            profile_file.display()
        )?;
        let profile = run_interactive_profiler(&ProfilerSession {
            non_interactive: true,
        })?;
        profile.save(&profile_file)?;
    }
    let profile = Profile::load(&profile_file)?;
    let key = if dry_run {
        EncryptionKey::from_passphrase("__dry_run__", b"cyclone-dry-run-v1")
    } else {
        encryption_key_from_passphrase(passphrase, true)?
    };
    let report = capture(CaptureOptions {
        profile: &profile,
        output: &output,
        encryption_key: &key,
        dry_run,
    })?;
    writeln!(
        io::stdout(),
        "capture {}: {} components",
        if report.dry_run { "dry-run" } else { "ok" },
        report.component_count
    )?;
    for w in report.warnings {
        writeln!(io::stdout(), "warning: {w}")?;
    }
    Ok(())
}

fn cmd_inspect(image: PathBuf) -> Result<()> {
    let report = inspect(&image)?;
    writeln!(io::stdout(), "image: {}", report.path)?;
    writeln!(io::stdout(), "platform: {}", report.host_platform)?;
    writeln!(io::stdout(), "components: {}", report.component_count)?;
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

fn cmd_restore(
    image: PathBuf,
    dry_run: bool,
    target_root: Option<PathBuf>,
    overwrite: bool,
    passphrase: Option<String>,
) -> Result<()> {
    let key = if dry_run {
        None
    } else {
        Some(encryption_key_from_passphrase(passphrase, true)?)
    };
    let report = restore(RestoreOptions {
        image: &image,
        encryption_key: key.as_ref(),
        target_root,
        dry_run,
        overwrite,
    })?;
    writeln!(
        io::stdout(),
        "restore {} ({} actions, {} files)",
        if report.dry_run { "dry-run" } else { "ok" },
        report.planned_actions.len(),
        report.applied_files
    )?;
    for action in report.planned_actions {
        writeln!(io::stdout(), "  - {action}")?;
    }
    for w in report.warnings {
        writeln!(io::stdout(), "warning: {w}")?;
    }
    Ok(())
}

fn cmd_provision(image: PathBuf, ext: String, target: String, dry_run: bool) -> Result<()> {
    if ext != "exe" {
        return Err(cyclone_core::CycloneError::Other(anyhow::anyhow!(
            "unknown extension `{ext}` (supported: exe)"
        )));
    }
    let bundle = ImageBundle::open(&image)?;
    let extension = ExeDevExtension::new();
    let plan = extension.plan(
        &bundle.manifest,
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

fn encryption_key_from_passphrase(
    passphrase: Option<String>,
    required: bool,
) -> Result<EncryptionKey> {
    let phrase = match passphrase {
        Some(p) if !p.is_empty() => p,
        _ if required => {
            return Err(cyclone_core::CycloneError::Crypto(
                "passphrase required (use --passphrase or CYCLONE_PASSPHRASE)".into(),
            ));
        }
        _ => String::new(),
    };
    Ok(EncryptionKey::from_passphrase(
        &phrase,
        b"cyclone-static-salt-v0",
    ))
}
