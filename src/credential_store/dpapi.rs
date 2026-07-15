//! Windows DPAPI credential store implementation.
//!
//! **Windows-only.** This module is compiled only when `cfg(windows)`.
//! Uses `windows-dpapi` `Scope::User` to encrypt/decrypt the agent token,
//! tying the secret to the current Windows user account.

#![cfg(windows)]

use std::path::{Path, PathBuf};

use windows_dpapi::{decrypt_data, encrypt_data, Scope};

use super::{CredentialError, CredentialStore};

/// Windows DPAPI-backed credential store.
///
/// Stores credentials as an encrypted blob (`credential.bin`) in the app directory.
/// The blob is encrypted with `CryptProtectData` using `Scope::User` — only the
/// same Windows user account can decrypt it.
pub struct DpapiCredentialStore {
    path: PathBuf,
}

impl DpapiCredentialStore {
    /// Create a new `DpapiCredentialStore` pointing at `app_dir/credential.bin`.
    pub fn new(app_dir: &Path) -> Self {
        Self {
            path: app_dir.join("credential.bin"),
        }
    }
}

impl CredentialStore for DpapiCredentialStore {
    fn save(&self, secret: &[u8]) -> Result<(), CredentialError> {
        let encrypted = encrypt_data(secret, Scope::User, None)
            .map_err(|e| CredentialError::Corrupt(anyhow::anyhow!(e)))?;
        std::fs::write(&self.path, encrypted)?;
        Ok(())
    }

    fn load(&self) -> Result<Vec<u8>, CredentialError> {
        // Check existence before decrypting — a missing file is distinct from a corrupt one.
        if !self.path.exists() {
            return Err(CredentialError::NotFound);
        }
        let ciphertext = std::fs::read(&self.path)?;
        let plaintext = decrypt_data(&ciphertext, Scope::User, None)
            .map_err(|e| CredentialError::Corrupt(anyhow::anyhow!(e)))?;
        Ok(plaintext)
    }
}
