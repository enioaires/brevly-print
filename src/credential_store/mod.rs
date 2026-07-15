//! Credential store: trait + cfg-gated platform implementations.
//!
//! On Windows: uses `DpapiCredentialStore` (encrypted via `windows-dpapi` `Scope::User`).
//! On Linux/non-Windows: uses `DevFileCredentialStore` (plaintext — DEV/TEST ONLY, not secure).
//!
//! Downstream plans fill the real logic in 01-02/T2.

pub mod error;

#[cfg(windows)]
mod dpapi;

#[cfg(not(windows))]
mod devfile;

pub use error::CredentialError;

use std::path::Path;

/// Abstraction over platform-specific credential storage.
///
/// Both implementations must never panic — errors are returned as typed `CredentialError`.
pub trait CredentialStore {
    /// Encrypt (or store) `secret` bytes and persist them to disk.
    fn save(&self, secret: &[u8]) -> Result<(), CredentialError>;

    /// Load and decrypt (or read) the stored secret bytes.
    ///
    /// Returns `Err(CredentialError::NotFound)` if the credential file does not exist.
    /// Returns `Err(CredentialError::Corrupt)` if the file is present but cannot be decrypted.
    fn load(&self) -> Result<Vec<u8>, CredentialError>;
}

/// Construct the platform credential store for `app_dir`.
///
/// - **Windows:** returns a `DpapiCredentialStore` (`credential.bin` encrypted with DPAPI Scope::User).
/// - **Non-Windows (Linux dev/test):** returns a `DevFileCredentialStore` (plaintext — NOT SECURE, never ships).
#[cfg(windows)]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore {
    dpapi::DpapiCredentialStore::new(app_dir)
}

#[cfg(not(windows))]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore {
    devfile::DevFileCredentialStore::new(app_dir)
}
