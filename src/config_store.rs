//! Configuration store backed by SQLite.
//!
//! **STUB** — implementations are filled in by plan 01-02/T1.
//! All function signatures here are the contracts downstream plans depend on.
//! The bodies use `todo!()` so they type-check but panic if called before 01-02 fills them.

use rusqlite::Connection;
use std::path::Path;

/// Open (or create) the SQLite database at `path` and run all pending migrations.
///
/// The caller must ensure the parent directory exists (`init_app_dir()` first).
///
/// # Errors
///
/// Returns a `rusqlite::Error` if the file cannot be opened or migrations fail.
pub fn open_and_migrate(path: &Path) -> rusqlite::Result<Connection> {
    let _ = path;
    todo!("01-02/T1: implement open_and_migrate with rusqlite_migration")
}

/// Set (upsert) a key/value pair in the `config` table.
///
/// # Errors
///
/// Returns a `rusqlite::Error` on SQL failure.
pub fn set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    let (_, _) = (key, value);
    let _ = conn;
    todo!("01-02/T1: implement config set")
}

/// Get the value for `key` from the `config` table.
///
/// Returns `Ok(None)` if the key does not exist.
///
/// # Errors
///
/// Returns a `rusqlite::Error` on SQL failure.
pub fn get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    let _ = (conn, key);
    todo!("01-02/T1: implement config get")
}
