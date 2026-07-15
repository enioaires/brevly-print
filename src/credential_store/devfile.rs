//! DEV/TEST ONLY — NOT A SECURE STORE. Never ships.
//!
//! Exists only to exercise the `CredentialStore` trait + error contract on Linux (D-24).
//! Product v1 is Windows-only; this Linux impl is dev/test parity only (D-24).
//!
//! **DO NOT use this in production.** The bytes are written as plaintext to a file with
//! no encryption. It is compiled exclusively under `#[cfg(not(windows))]` and is never
//! included in the Windows binary.

#![cfg(not(windows))]

use std::path::{Path, PathBuf};

use super::{CredentialError, CredentialStore};

/// Plain-file credential store for Linux dev/test use only.
///
/// Writes raw bytes to `credential.bin` in the app directory without encryption.
/// Implements the same `CredentialStore` trait as `DpapiCredentialStore` so the
/// error contract and trait API can be tested on Linux without DPAPI.
///
/// # Security
///
/// **NOT SECURE. DEV/TEST ONLY. Never ships as a product credential store.**
pub struct DevFileCredentialStore {
    path: PathBuf,
}

impl DevFileCredentialStore {
    /// Create a new `DevFileCredentialStore` pointing at `app_dir/credential.bin`.
    pub fn new(app_dir: &Path) -> Self {
        Self {
            path: app_dir.join("credential.bin"),
        }
    }
}

impl CredentialStore for DevFileCredentialStore {
    fn save(&self, secret: &[u8]) -> Result<(), CredentialError> {
        std::fs::write(&self.path, secret)?;
        Ok(())
    }

    fn load(&self) -> Result<Vec<u8>, CredentialError> {
        // Check existence before reading — same contract as DPAPI impl.
        if !self.path.exists() {
            return Err(CredentialError::NotFound);
        }
        let bytes = std::fs::read(&self.path)?;
        if bytes.is_empty() {
            // Empty file treated as corrupt (matches DPAPI decrypt failure semantics).
            return Err(CredentialError::Corrupt(anyhow::anyhow!(
                "credential file is empty"
            )));
        }
        Ok(bytes)
    }
}
