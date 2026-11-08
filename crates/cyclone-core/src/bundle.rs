use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::artifacts::CapturedPayload;
use crate::crypto::{decrypt_bytes, encrypt_bytes, EncryptionKey, KdfParams};
use crate::error::{CycloneError, Result};
use crate::manifest::ImageManifest;

const HEADER_NAME: &str = "header.json";
const CIPHERTEXT_NAME: &str = "bundle.bin.enc";
pub const AAD: &[u8] = b"cyclone-image-v1";
pub const MAGIC: &str = "cyclone";
pub const FORMAT_VERSION: u32 = 1;

/// Unencrypted envelope. Contains KDF parameters only — no manifest, paths, or file bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageHeader {
    pub magic: String,
    pub format_version: u32,
    pub aead: String,
    pub kdf: KdfParams,
}

/// Decrypted image contents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InnerBundle {
    pub manifest: ImageManifest,
    pub payload: CapturedPayload,
}

/// On-disk encrypted image (a directory).
pub struct ImageBundle {
    pub root: PathBuf,
    pub header: ImageHeader,
}

impl ImageBundle {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_dir() {
            return Err(CycloneError::ImageNotFound(path.display().to_string()));
        }
        let header_path = path.join(HEADER_NAME);
        let raw = fs::read_to_string(&header_path).map_err(|_| {
            CycloneError::ImageNotFound(format!("missing {HEADER_NAME} in {}", path.display()))
        })?;
        let header: ImageHeader = serde_json::from_str(&raw)?;
        if header.magic != MAGIC {
            return Err(CycloneError::InvalidManifest(format!(
                "not a cyclone image (magic `{}`)",
                header.magic
            )));
        }
        if header.format_version != FORMAT_VERSION {
            return Err(CycloneError::InvalidManifest(format!(
                "unsupported image format {}",
                header.format_version
            )));
        }
        let ct_path = path.join(CIPHERTEXT_NAME);
        if !ct_path.is_file() {
            return Err(CycloneError::ImageNotFound(format!(
                "missing {CIPHERTEXT_NAME} in {}",
                path.display()
            )));
        }
        Ok(Self {
            root: path.to_path_buf(),
            header,
        })
    }

    pub fn write(
        path: &Path,
        inner: &InnerBundle,
        passphrase: &str,
        fast_kdf: bool,
    ) -> Result<Self> {
        if path.exists() {
            return Err(CycloneError::User(format!(
                "output already exists: {}",
                path.display()
            )));
        }
        fs::create_dir_all(path)?;
        let (key, kdf) = EncryptionKey::derive_new(passphrase, fast_kdf)?;
        let plaintext = serde_json::to_vec(inner)?;
        let ciphertext = encrypt_bytes(&plaintext, &key, AAD)?;
        let header = ImageHeader {
            magic: MAGIC.into(),
            format_version: FORMAT_VERSION,
            aead: "chacha20poly1305".into(),
            kdf,
        };
        fs::write(
            path.join(HEADER_NAME),
            serde_json::to_string_pretty(&header)?,
        )?;
        fs::write(path.join(CIPHERTEXT_NAME), ciphertext)?;
        Ok(Self {
            root: path.to_path_buf(),
            header,
        })
    }

    pub fn decrypt(&self, passphrase: &str) -> Result<InnerBundle> {
        let key = EncryptionKey::derive(passphrase, &self.header.kdf)?;
        let ct = fs::read(self.root.join(CIPHERTEXT_NAME))
            .map_err(|_| CycloneError::ImageNotFound("encrypted bundle missing".into()))?;
        let plaintext = decrypt_bytes(&ct, &key, AAD)?;
        Ok(serde_json::from_slice(&plaintext)?)
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Concatenate every file in the image directory (for leak tests).
    pub fn raw_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for name in [HEADER_NAME, CIPHERTEXT_NAME] {
            out.extend_from_slice(&fs::read(self.root.join(name))?);
        }
        Ok(out)
    }
}
