//! Integration tests for the config store (SQLite-backed key/value store).
//!
//! Portable: runs on Linux and Windows (no Windows-API dependency).

use brevly_print::config_store;

/// Verify that `open_and_migrate` initializes schema v1 (3 tables) and sets `user_version = 1`.
#[test]
fn test_schema_v1_and_user_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let conn = config_store::open_and_migrate(&db_path).expect("open_and_migrate should succeed");

    // Assert user_version pragma == 1
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("PRAGMA user_version should succeed");
    assert_eq!(user_version, 1, "user_version must be 1 after migration v1");

    // Assert all 3 schema-v1 tables exist
    for table in &["config", "printed_jobs", "retry_queue"] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("sqlite_master query should succeed");
        assert_eq!(count, 1, "table '{}' must exist after migration v1", table);
    }
}

/// Verify that `open_and_migrate` is idempotent: re-opening an already-migrated db
/// must not fail and must leave `user_version` at 1.
#[test]
fn test_open_and_migrate_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");

    // First open — applies migration v1
    let conn1 = config_store::open_and_migrate(&db_path).expect("first open_and_migrate should succeed");
    let v1: i64 = conn1
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("first PRAGMA user_version");
    assert_eq!(v1, 1);
    drop(conn1);

    // Second open — must be a no-op (already at v1)
    let conn2 = config_store::open_and_migrate(&db_path).expect("second open_and_migrate should succeed");
    let v2: i64 = conn2
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("second PRAGMA user_version");
    assert_eq!(v2, 1, "user_version must still be 1 after second open (idempotent)");
}

/// Verify that `set` and `get` round-trip through the `config` table.
#[test]
fn test_write_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let conn = config_store::open_and_migrate(&db_path).expect("open_and_migrate should succeed");

    config_store::set(&conn, "printer_name", "EPSON_TM_T20").expect("set should succeed");
    let got = config_store::get(&conn, "printer_name").expect("get should succeed");
    assert_eq!(got, Some("EPSON_TM_T20".to_string()), "set→get must round-trip");

    // Verify upsert: overwriting an existing key must update the value
    config_store::set(&conn, "printer_name", "BIXOLON_SRP").expect("set (upsert) should succeed");
    let updated = config_store::get(&conn, "printer_name").expect("get after upsert should succeed");
    assert_eq!(updated, Some("BIXOLON_SRP".to_string()), "upsert must update the value");
}

/// Verify that `get` on an absent key returns `Ok(None)`, not an error.
#[test]
fn test_get_absent_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let conn = config_store::open_and_migrate(&db_path).expect("open_and_migrate should succeed");

    let missing = config_store::get(&conn, "nonexistent_key").expect("get on absent key should return Ok, not Err");
    assert_eq!(missing, None, "absent key must return Ok(None)");
}
