//! Integration tests for the config store (SQLite-backed key/value store).
//!
//! Portable: runs on Linux and Windows (no Windows-API dependency).

use brevly_print::config_store;

/// Verify that `open_and_migrate` initializes all schema tables and sets `user_version = 2`
/// after migration v2 (Phase 6 adds 'printing' intermediate status for crash recovery).
///
/// This test is RED until Wave 1 adds migration v2 — that is intentional Nyquist behavior.
#[test]
fn test_schema_and_user_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let conn = config_store::open_and_migrate(&db_path).expect("open_and_migrate should succeed");

    // Assert user_version pragma == 2 (Wave 1 migration v2 must advance it from 1 → 2)
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("PRAGMA user_version should succeed");
    assert_eq!(user_version, 2, "user_version must be 2 after migration v2 (Phase 6)");

    // Assert all 3 schema tables exist (printed_jobs is recreated in v2, not dropped permanently)
    for table in &["config", "printed_jobs", "retry_queue"] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("sqlite_master query should succeed");
        assert_eq!(count, 1, "table '{}' must exist after migration v2", table);
    }
}

/// Verify that `open_and_migrate` is idempotent: re-opening an already-migrated db
/// must not fail and must leave `user_version` at 2 (after Wave 1 adds migration v2).
///
/// This test is RED until Wave 1 adds migration v2 — that is intentional Nyquist behavior.
#[test]
fn test_open_and_migrate_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");

    // First open — applies migrations up to v2
    let conn1 = config_store::open_and_migrate(&db_path).expect("first open_and_migrate should succeed");
    let v1: i64 = conn1
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("first PRAGMA user_version");
    assert_eq!(v1, 2, "user_version must be 2 after first open (migrations v1 + v2 applied)");
    drop(conn1);

    // Second open — must be a no-op (already at v2)
    let conn2 = config_store::open_and_migrate(&db_path).expect("second open_and_migrate should succeed");
    let v2: i64 = conn2
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("second PRAGMA user_version");
    assert_eq!(v2, 2, "user_version must still be 2 after second open (idempotent)");
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

/// Verify that `printed_jobs` accepts the new `'printing'` intermediate status after
/// migration v2 (D-01: v2 adds CHECK(status IN ('pending','printing','printed','failed'))).
///
/// The v1 CHECK constraint only allows 'pending' and 'printed', so this INSERT would
/// fail under v1.  After v2 it must succeed — proving the table-recreation migration
/// correctly expanded the allowed values.
///
/// This test is RED until Wave 1 adds migration v2 — that is intentional Nyquist behavior.
#[test]
fn printed_jobs_accepts_printing_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let conn = config_store::open_and_migrate(&db_path).expect("open_and_migrate should succeed");

    let result = conn.execute(
        "INSERT INTO printed_jobs (job_id, status) VALUES ('j1', 'printing')",
        [],
    );
    assert!(
        result.is_ok(),
        "migration v2 must allow status='printing' in printed_jobs (D-01 / RES-04); \
         got: {:?}",
        result
    );
}
