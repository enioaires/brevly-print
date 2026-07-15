#![cfg(target_os = "windows")]
//! Integration tests for the real DPAPI credential store (Windows only).
//!
//! These tests require a real Windows user session with a valid user profile so
//! DPAPI `Scope::User` can encrypt and decrypt. They run on:
//!   - The owner's Windows dev box
//!   - GitHub Actions `windows-latest` runner (has a real user session)
//!
//! **NOT compiled or run on Linux** (cfg-gate at the top of this file).
//!
//! T-1-01 (threat mitigation): proves DPAPI Scope::User round-trip and Corrupt error on
//! bad blob — the exact paths Phase 2 uses to handle credential failure without panicking.

use brevly_print::credential_store::{credential_store, CredentialError, CredentialStore};

/// Verify a full DPAPI encrypt → write → read → decrypt round-trip.
#[test]
fn test_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = credential_store(dir.path());
    let secret = b"dpapi-test-secret-12345";
    store.save(secret).expect("save should succeed");
    let loaded = store.load().expect("load should succeed");
    assert_eq!(loaded, secret);
}

/// Verify that a missing credential file returns `CredentialError::NotFound`.
#[test]
fn test_missing_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = credential_store(dir.path());
    // No save() called — file does not exist.
    let result = store.load();
    assert!(
        matches!(result, Err(CredentialError::NotFound)),
        "expected NotFound, got: {result:?}"
    );
}

/// Verify that corrupt bytes in credential.bin return `CredentialError::Corrupt`.
///
/// Proves T-1-01: undecryptable DPAPI blob → typed error, never panic.
#[test]
fn test_corrupt_blob() {
    let dir = tempfile::tempdir().expect("tempdir");
    let credential_path = dir.path().join("credential.bin");
    // Write garbage bytes that DPAPI cannot decrypt.
    std::fs::write(&credential_path, b"not-a-valid-dpapi-blob").expect("write corrupt blob");
    let store = credential_store(dir.path());
    let result = store.load();
    assert!(
        matches!(result, Err(CredentialError::Corrupt(_))),
        "expected Corrupt, got: {result:?}"
    );
}
