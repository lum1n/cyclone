//! # cyclone-core
//!
//! Library for building and applying **Cyclone developer-setup images** on macOS and Linux.
//!
//! ## Concepts
//!
//! - **Profile** (`profile.toml`): user-editable list of paths and tooling metadata to capture.
//! - **Image bundle**: directory containing `manifest.json` (plaintext metadata) and
//!   `payload.bin.enc` (ChaCha20-Poly1305 encrypted artifact blob).
//! - **Encryption**: symmetric key derived from a user passphrase; Cyclone never stores the key.
//!
//! Cloud SDKs stay out of this crate. VM/cloud flows use the [`Extension`] trait in separate crates.
//!
//! ## Example (library)
//!
//! ```no_run
//! use cyclone_core::{capture, inspect, CaptureOptions, EncryptionKey, Profile};
//! use std::path::Path;
//!
//! # fn main() -> cyclone_core::Result<()> {
//! let profile = Profile::default();
//! let key = EncryptionKey::from_passphrase("example", b"salt");
//! let _ = capture(CaptureOptions {
//!     profile: &profile,
//!     output: Path::new("/tmp/image"),
//!     encryption_key: &key,
//!     dry_run: true,
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
pub mod platform;
pub mod profile;
pub mod profiler;
pub mod restore;

pub use artifacts::{ArtifactKind, CapturedArtifact, CapturedPayload};
pub use bundle::{ImageBundle, ImageLayout};
pub use capture::{capture, CaptureOptions, CaptureReport};
pub use crypto::{decrypt_bundle, encrypt_bundle, EncryptionKey};
pub use error::{CycloneError, Result};
pub use extension::{Extension, ProvisionPlan, ProvisionStep, ProvisionTarget};
pub use inspect::{inspect, InspectReport};
pub use manifest::{ImageManifest, ManifestComponent, SecretReference};
pub use platform::Platform;
pub use profile::{Profile, ProfilePaths, ToolDeclaration};
pub use profiler::{discover_defaults, run_interactive_profiler, ProfilerSession};
pub use restore::{restore, RestoreOptions, RestoreReport};
