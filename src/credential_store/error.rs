//! Portable credential store error type.
//!
//! This module is **always compiled** (no cfg-gate) so both the Linux dev impl and the
//! Windows DPAPI impl share the same typed error contract — testable on both platforms.

use thiserror::Error;

/// Errors that can occur when reading or writing the credential store.
///
/// This enum never panics — callers can match on variants to determine the appropriate
/// recovery action (e.g. re-enter the activation flow on `NotFound` or `Corrupt`).
#[derive(Error, Debug)]
pub enum CredentialError {
    /// The credential file does not exist (agent has not been activated yet, or
    /// the file was manually deleted).
    #[error("Credential file not found — agent needs activation")]
    NotFound,

    /// The credential file exists but cannot be decrypted.
    ///
    /// On Windows this typically indicates a DPAPI key mismatch after a Windows
    /// reinstall or SID change (pitfall M7). The agent should re-enter the
    /// activation flow.
    #[error("Credential file is corrupt or was encrypted by a different user (DPAPI key mismatch)")]
    Corrupt(#[source] anyhow::Error),

    /// An I/O error occurred while reading or writing the credential file.
    #[error("I/O error accessing credential file")]
    Io(#[from] std::io::Error),
}
