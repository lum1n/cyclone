//! # cyclone-core
//!
//! Library for building and applying **Cyclone developer-setup images** on macOS and Linux.
//!
//! ## Concepts
//!
//! - **Profile** (`profile.toml`): user-editable list of components with per-OS logical paths.
//!   `cyclone init` / `profile --edit` scan `~/.config` and `~/` so any discovered
//!   path can be added; SSH keys, keychains, browser data, and large caches stay opt-in.
//! - **Image bundle**: directory containing `header.json` (KDF envelope) and
//!   `bundle.bin.enc` (ChaCha20-Poly1305 ciphertext of the entire manifest + artifacts).
//! - **Encryption**: Argon2id + a user-held passphrase; Cyclone never stores the key.
//!   The default (and only) on-disk format is encrypted. Plaintext images are not produced.
//!
//! Cloud SDKs stay out of this crate. VM/cloud flows use the [`Extension`] trait in separate crates.
//!
//! ## Example (library)
//!
//! ```no_run
//! use cyclone_core::{capture, CaptureOptions, Platform, Profile};
//! use std::path::PathBuf;
//!
//! # fn main() -> cyclone_core::Result<()> {
//! let profile = Profile::default();
//! let _ = capture(CaptureOptions {
//!     profile: &profile,
//!     output: std::path::Path::new("/tmp/image.cyclone"),
//!     passphrase: None,
//!     home: PathBuf::from("/tmp/home"),
//!     platform: Platform::Linux,
//!     dry_run: true,
//!     fast_kdf: true,
//!     packages: Default::default(),
//! })?;
//! # Ok(())
//! # }
//! ```

pub mod artifacts;
pub mod bundle;
pub mod capture;
pub mod crypto;
pub mod error;
pub mod extension;
pub mod inspect;
pub mod manifest;
pub mod packages;
pub mod paths;
pub mod platform;
pub mod profile;
pub mod profiler;
pub mod restore;
pub mod scan;

pub use artifacts::{ArtifactKind, CapturedArtifact, CapturedPayload};
pub use bundle::{ImageBundle, ImageHeader, InnerBundle};
pub use capture::{capture, CaptureOptions, CaptureReport};
pub use crypto::{decrypt_bytes, encrypt_bytes, EncryptionKey, KdfParams};
pub use error::{CycloneError, Result};
pub use extension::{
    ApplyOptions, Extension, ProvisionPlan, ProvisionResult, ProvisionStep, ProvisionTarget,
};
pub use inspect::{inspect, inspect_envelope, InspectReport};
pub use manifest::{ImageManifest, ManifestComponent, SecretReference};
pub use packages::{
    CommandProbe, HostEnv, PackageCaptureMode, PackageCaptureOptions, PackageDeclaration,
    PackageManager, PackageRestoreOptions, PackageSpec, ScriptedCommands,
};
pub use paths::{default_home, expand_tilde, mapped_dest, to_logical};
pub use platform::{command_on_path, Distro, Platform};
pub use profile::{ComponentKind, OptInReason, Profile, ProfileComponent, ProfilePaths};
pub use profiler::{
    apply_selection_line, build_profile, discover_defaults, run_profiler, CatalogEntry,
    ProfilerSession, CATALOG,
};
pub use restore::{restore, RestoreAction, RestoreActionKind, RestoreOptions, RestoreReport};
pub use scan::{classify, scan_home, ScanHit};
