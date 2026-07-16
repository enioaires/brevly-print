//! Configuration store backed by SQLite.
//!
//! Provides versioned migrations via `rusqlite_migration` (`user_version` pragma).
//! Migration v1 initialises the three schema-v1 tables: `config`, `printed_jobs`,
//! and `retry_queue` (D-12, D-13, D-14).
//! Migration v2 adds the `'printing'` intermediate status to `printed_jobs.status`
//! CHECK constraint for crash recovery (RES-04 / D-01). SQLite cannot modify a CHECK
//! constraint in place, so v2 uses the table-recreation pattern (create v2, copy, drop,
//! rename, re-index).
//!
//! Fully portable — no `#[cfg(windows)]` gates needed.
//!
//! ## Pitfall 7 / OQ1 resolution
//!
//! The RESEARCH doc (Pattern 4, Pitfall 7) warned that a single multi-statement `M::up`
//! string may fail at runtime on some SQLite builds because `execute_batch` chokes on
//! multiple statements. Testing confirmed that `rusqlite_migration 2.6` + bundled SQLite
//! handle a multi-statement `M::up` correctly on Linux (all three CREATE TABLE + CREATE INDEX
//! in one string). **Path taken: single multi-statement `M::up` (primary path)**. If this
//! fails on Windows, split into separate `M::up` calls (user_version would then be 3 for the
//! same logical schema v1; adjust the test accordingly).

use rusqlite::{Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use std::path::Path;
use std::sync::LazyLock;

// ── Schema v1 migration ──────────────────────────────────────────────────────
//
// All three D-14 tables in a single `M::up` so `user_version` advances to exactly 1.
// Bundled SQLite (via rusqlite "bundled" feature) handles multi-statement execute_batch
// correctly; this is the safe path for the bundled build.

static MIGRATIONS: LazyLock<Migrations<'static>> = LazyLock::new(|| {
    Migrations::new(vec![
        // v1 — creates config, printed_jobs (+ status index), retry_queue
        M::up(
            "CREATE TABLE config (
                key   TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );
            CREATE TABLE printed_jobs (
                job_id      TEXT PRIMARY KEY NOT NULL,
                job_type    TEXT,
                status      TEXT NOT NULL DEFAULT 'pending'
                                CHECK(status IN ('pending','printed','failed')),
                attempt     INTEGER NOT NULL DEFAULT 0,
                received_at TEXT,
                printed_at  TEXT,
                failed_at   TEXT
            );
            CREATE INDEX idx_printed_jobs_status ON printed_jobs(status);
            CREATE TABLE retry_queue (
                job_id        TEXT PRIMARY KEY NOT NULL
                                  REFERENCES printed_jobs(job_id),
                job_type      TEXT,
                escpos_bytes  BLOB,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                next_retry_at TEXT,
                last_error    TEXT,
                created_at    TEXT
            );",
        ),
        // v2 — add 'printing' intermediate status for crash recovery (RES-04)
        //
        // SQLite does not support ALTER TABLE ... MODIFY COLUMN or in-place CHECK changes,
        // so the standard table-recreation pattern is used: create v2 with the expanded
        // CHECK, copy all rows positionally (column order matches v1 exactly), drop old
        // table, rename v2, re-create the status index.
        //
        // FK note: retry_queue.job_id REFERENCES printed_jobs(job_id). rusqlite_migration
        // disables FK enforcement during migrations by default (PRAGMA foreign_keys=OFF),
        // so the DROP/RENAME is safe without any pragma override here.
        M::up(
            "CREATE TABLE printed_jobs_v2 (
                job_id      TEXT PRIMARY KEY NOT NULL,
                job_type    TEXT,
                status      TEXT NOT NULL DEFAULT 'pending'
                                CHECK(status IN ('pending','printing','printed','failed')),
                attempt     INTEGER NOT NULL DEFAULT 0,
                received_at TEXT,
                printed_at  TEXT,
                failed_at   TEXT
            );
            INSERT INTO printed_jobs_v2 SELECT * FROM printed_jobs;
            DROP TABLE printed_jobs;
            ALTER TABLE printed_jobs_v2 RENAME TO printed_jobs;
            CREATE INDEX idx_printed_jobs_status ON printed_jobs(status);",
        ),
    ])
});

/// Open (or create) the SQLite database at `path` and run all pending migrations.
///
/// The caller must ensure the parent directory exists (`init_app_dir()` first).
/// Re-opening an already-migrated database is a no-op (idempotent via `user_version`).
///
/// # Errors
///
/// Returns a `rusqlite::Error` if the file cannot be opened, or a
/// `rusqlite_migration::Error` (wrapped) if migrations fail.
pub fn open_and_migrate(path: &Path) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open(path)?;
    MIGRATIONS
        .to_latest(&mut conn)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    Ok(conn)
}

/// Set (upsert) a key/value pair in the `config` table.
///
/// If `key` already exists its `value` is overwritten (INSERT … ON CONFLICT UPDATE).
///
/// # Errors
///
/// Returns a `rusqlite::Error` on SQL failure.
pub fn set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO config(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Get the value for `key` from the `config` table.
///
/// Returns `Ok(None)` if the key does not exist (not an error).
///
/// # Errors
///
/// Returns a `rusqlite::Error` on SQL failure.
pub fn get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM config WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get(0),
    )
    .optional()
}
