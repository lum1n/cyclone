use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CycloneError, Result};

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Argon2id parameters stored in the image envelope (not secret).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KdfParams {
    pub algorithm: String,
    pub salt_hex: String,
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    pub fn production(salt_hex: String) -> Self {
        Self {
            algorithm: "argon2id".into(),
            salt_hex,
            m_cost_kib: 64 * 1024,
            t_cost: 3,
            p_cost: 1,
        }
    }

    /// Low-memory parameters for tests and CI. Never used by the CLI unless `--fast-kdf`.
    pub fn fast_for_tests(salt_hex: String) -> Self {
        Self {
            algorithm: "argon2id".into(),
            salt_hex,
            m_cost_kib: 8,
            t_cost: 1,
            p_cost: 1,
        }
    }
}

/// User-held symmetric key (32 bytes). Zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct EncryptionKey {
    bytes: [u8; KEY_LEN],
}

impl EncryptionKey {
    pub fn derive(passphrase: &str, kdf: &KdfParams) -> Result<Self> {
        if kdf.algorithm != "argon2id" {
            return Err(CycloneError::Crypto(format!(
                "unsupported kdf `{}`",
                kdf.algorithm
            )));
        }
        let salt = hex::decode(&kdf.salt_hex)
            .map_err(|e| CycloneError::Crypto(format!("invalid kdf salt: {e}")))?;
        if salt.len() < 8 {
            return Err(CycloneError::Crypto("kdf salt is too short".into()));
        }
        let params = Params::new(kdf.m_cost_kib, kdf.t_cost, kdf.p_cost, Some(KEY_LEN))
            .map_err(|e| CycloneError::Crypto(e.to_string()))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut bytes = [0u8; KEY_LEN];
        argon2
            .hash_password_into(passphrase.as_bytes(), &salt, &mut bytes)
            .map_err(|e| CycloneError::Crypto(e.to_string()))?;
        Ok(Self { bytes })
    }

    pub fn derive_new(passphrase: &str, fast: bool) -> Result<(Self, KdfParams)> {
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let kdf = if fast {
            KdfParams::fast_for_tests(hex::encode(salt))
        } else {
            KdfParams::production(hex::encode(salt))
        };
        let key = Self::derive(passphrase, &kdf)?;
        Ok((key, kdf))
    }

    pub fn fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.bytes);
        hex::encode(&hasher.finalize()[..8])
    }
}

impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionKey")
            .field("fingerprint", &self.fingerprint())
            .finish()
    }
}

pub fn encrypt_bytes(plaintext: &[u8], key: &EncryptionKey, aad: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new_from_slice(&key.bytes)
        .map_err(|e| CycloneError::Crypto(e.to_string()))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|e| CycloneError::Crypto(e.to_string()))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_bytes(blob: &[u8], key: &EncryptionKey, aad: &[u8]) -> Result<Vec<u8>> {
    if blob.len() < NONCE_LEN {
        return Err(CycloneError::Crypto("ciphertext too short".into()));
    }
    let (nonce_bytes, ct) = blob.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new_from_slice(&key.bytes)
        .map_err(|e| CycloneError::Crypto(e.to_string()))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, Payload { msg: ct, aad })
        .map_err(|_| CycloneError::Crypto("incorrect passphrase or corrupted image".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let (key, kdf) = EncryptionKey::derive_new("test-passphrase", true).unwrap();
        let aad = b"cyclone-image-v1";
        let pt = br#"{"hello":"world"}"#;
        let ct = encrypt_bytes(pt, &key, aad).unwrap();
        let back = decrypt_bytes(&ct, &key, aad).unwrap();
        assert_eq!(back, pt);
        let other = EncryptionKey::derive("wrong", &kdf).unwrap();
        assert!(decrypt_bytes(&ct, &other, aad).is_err());
    }
}
