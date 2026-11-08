use std::fs;
use std::path::{Path, PathBuf};

use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::crypto::{decrypt_bundle, encrypt_bundle, EncryptionKey};
use crate::error::{CycloneError, Result};
use crate::manifest::ImageManifest;

const MANIFEST_NAME: &str = "manifest.json";
const PAYLOAD_NAME: &str = "payload.bin.enc";
const SALT_NAME: &str = "encryption.salt";
const AAD: &[u8] = b"cyclone-image-v1";

/// On-disk layout for a Cyclone image (directory bundle).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageLayout {
    pub root: PathBuf,
}

pub struct ImageBundle {
    layout: ImageLayout,
    pub manifest: ImageManifest,
}

impl ImageBundle {
    pub fn create_at(path: &Path, manifest: ImageManifest) -> Result<Self> {
        fs::create_dir_all(path)?;
        let layout = ImageLayout {
            root: path.to_path_buf(),
        };
        let bundle = Self { layout, manifest };
        bundle.write_manifest()?;
        Ok(bundle)
    }

    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_dir() {
            return Err(CycloneError::ImageNotFound(path.display().to_string()));
        }
        let manifest_path = path.join(MANIFEST_NAME);
        let raw = fs::read_to_string(&manifest_path).map_err(|_| {
            CycloneError::ImageNotFound(format!("missing manifest in {}", path.display()))
        })?;
        let manifest: ImageManifest = serde_json::from_str(&raw)?;
        Ok(Self {
            layout: ImageLayout {
                root: path.to_path_buf(),
            },
            manifest,
        })
    }

    pub fn path(&self) -> &Path {
        &self.layout.root
    }

    pub fn write_manifest(&self) -> Result<()> {
        let raw = serde_json::to_string_pretty(&self.manifest)?;
        fs::write(self.layout.root.join(MANIFEST_NAME), raw)?;
        Ok(())
    }

    pub fn write_encrypted_payload(&self, plaintext: &[u8], key: &EncryptionKey) -> Result<()> {
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        fs::write(self.layout.root.join(SALT_NAME), hex::encode(salt))?;
        let ciphertext = encrypt_bundle(plaintext, key, AAD)?;
        fs::write(self.layout.root.join(PAYLOAD_NAME), ciphertext)?;
        Ok(())
    }

    pub fn read_encrypted_payload(&self, key: &EncryptionKey) -> Result<Vec<u8>> {
        let ct = fs::read(self.layout.root.join(PAYLOAD_NAME))
            .map_err(|_| CycloneError::ImageNotFound("encrypted payload missing".to_string()))?;
        decrypt_bundle(&ct, key, AAD)
    }
}
