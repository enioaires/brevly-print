//! Integration tests for the `CredentialStore` trait contract via `DevFileCredentialStore`.
//!
//! **Linux-provable trait contract** — NOT Windows-gated.
//! These tests exercise the `CredentialStore` trait + `CredentialError` error contract
//! using the Linux dev implementation (`DevFileCredentialStore`) on any platform.
//!
//! This is the portable counterpart to `credential_store_test.rs` (which tests the real
//! DPAPI path, Windows-only). The trait contract — NotFound / successful round-trip —
//! is provable here without DPAPI.
//!
//! Added during cross-platform re-plan (2026-07-15, not in the original VALIDATION map).

use brevly_print::credential_store::{credential_store, CredentialError, CredentialStore};

/// Verify a full save → load round-trip through the platform credential store trait.
///
/// On Linux: uses `DevFileCredentialStore` (plaintext, DEV/TEST ONLY).
/// On Windows: uses `DpapiCredentialStore` (DPAPI encrypted).
#[test]
fn test_trait_contract_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = credential_store(dir.path());
    let secret = b"trait-contract-test-secret";
    store.save(secret).expect("save should succeed");
    let loaded = store.load().expect("load should succeed");
    assert_eq!(loaded.as_slice(), secret);
}

/// Verify that load() returns `CredentialError::NotFound` when no credential exists.
///
/// Portable: works on Linux (DevFile) and Windows (DPAPI).
#[test]
fn test_trait_contract_not_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = credential_store(dir.path());
    // No save() called.
    let result = store.load();
    assert!(
        matches!(result, Err(CredentialError::NotFound)),
        "expected NotFound, got: {result:?}"
    );
}
