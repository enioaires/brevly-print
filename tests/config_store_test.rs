//! Integration tests for the config store (SQLite-backed key/value store).
//!
//! **Wave-0 scaffold** — these tests compile against the real API signatures but are
//! marked `#[ignore]` because the implementations use `todo!()` until plan 01-02/T1
//! fills them in. Un-ignoring and adding assertions is done in 01-02/T1.
//!
//! Portable: runs on Linux and Windows (no Windows-API dependency).

use brevly_print::config_store;

/// Verify that `open_and_migrate` initializes schema v1 (3 tables) and sets `user_version = 1`.
///
/// Filled in 01-02/T1.
#[test]
#[ignore = "implementation pending 01-02/T1"]
fn test_schema_v1_and_user_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let _conn = config_store::open_and_migrate(&db_path).expect("open_and_migrate should succeed");
    // Assertions added in 01-02/T1:
    // - user_version pragma == 1
    // - tables: config, printed_jobs, retry_queue exist
}

/// Verify that `set` and `get` round-trip through the `config` table.
///
/// Filled in 01-02/T1.
#[test]
#[ignore = "implementation pending 01-02/T1"]
fn test_write_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let conn = config_store::open_and_migrate(&db_path).expect("open_and_migrate should succeed");
    config_store::set(&conn, "test_key", "test_value").expect("set should succeed");
    let got = config_store::get(&conn, "test_key").expect("get should succeed");
    assert_eq!(got, Some("test_value".to_string()));
    // Also verify missing key returns None:
    let missing = config_store::get(&conn, "nonexistent").expect("get nonexistent should succeed");
    assert_eq!(missing, None);
}
