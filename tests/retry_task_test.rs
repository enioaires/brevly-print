//! RED stubs for Phase 6 resilience requirements: RES-01, RES-02, RES-04.
//!
//! Wave 0 scaffold — these tests assert the observable SQL invariants that
//! Wave 1/2 production code must satisfy.  Tests that call not-yet-existing
//! production functions (`run_retry_task`) are gated behind `#[ignore]` so the
//! file compiles today; their bodies are `todo!()` placeholders.
//!
//! Portable: runs on Linux and Windows (no Windows-API dependency in this file).

use rusqlite::Connection;

// ── Schema helper ────────────────────────────────────────────────────────────

/// Open an in-memory SQLite connection and create the schema-v2 tables
/// (`printed_jobs` + `retry_queue`) as they will look after migration v2.
///
/// Note: the `printed_jobs` table here intentionally omits the v2 CHECK constraint
/// so tests can seed arbitrary status values (e.g. 'printing') without triggering
/// the constraint — the constraint is tested via `config_store_test.rs`.
fn make_test_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory DB");
    conn.execute_batch(
        "CREATE TABLE printed_jobs (
            job_id      TEXT PRIMARY KEY NOT NULL,
            job_type    TEXT,
            status      TEXT NOT NULL DEFAULT 'pending',
            attempt     INTEGER NOT NULL DEFAULT 0,
            received_at TEXT,
            printed_at  TEXT,
            failed_at   TEXT
        );
        CREATE TABLE retry_queue (
            job_id        TEXT PRIMARY KEY NOT NULL,
            job_type      TEXT,
            escpos_bytes  BLOB,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_retry_at TEXT,
            last_error    TEXT,
            created_at    TEXT
        );",
    )
    .expect("create test schema");
    conn
}

// ── RES-04: Crash recovery (D-05) ────────────────────────────────────────────

/// Assert the D-05 crash-recovery query returns exactly the orphaned `'printing'`
/// row — the one whose `job_id` is NOT already in `retry_queue`.
///
/// Seeds:
///   - job-orphan: status='printing', NOT in retry_queue  → must be returned
///   - job-queued:  status='printing', IS  in retry_queue  → must NOT be returned
///
/// RES-04: a process crash mid-print must not silently lose a ticket.
#[test]
fn crash_recovery_selects_orphaned_printing_rows() {
    let conn = make_test_conn();

    // Seed printed_jobs with two 'printing' rows
    conn.execute_batch(
        "INSERT INTO printed_jobs (job_id, job_type, status)
             VALUES ('job-orphan', 'pedido', 'printing');
         INSERT INTO printed_jobs (job_id, job_type, status)
             VALUES ('job-queued', 'despacho', 'printing');",
    )
    .expect("seed printed_jobs");

    // Seed retry_queue with only 'job-queued' (it survived the crash with bytes saved)
    conn.execute_batch(
        "INSERT INTO retry_queue (job_id, job_type, attempt_count, next_retry_at, created_at)
             VALUES ('job-queued', 'despacho', 1, datetime('now','+30 seconds'), datetime('now'));",
    )
    .expect("seed retry_queue");

    // D-05 query: find 'printing' rows whose job_id is NOT yet in retry_queue
    let mut stmt = conn
        .prepare(
            "SELECT job_id, job_type FROM printed_jobs
             WHERE status = 'printing'
               AND job_id NOT IN (SELECT job_id FROM retry_queue)",
        )
        .expect("prepare D-05 query");

    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();

    assert_eq!(rows.len(), 1, "D-05 query must return exactly one orphan row");
    assert_eq!(rows[0].0, "job-orphan", "the orphan must be 'job-orphan'");
    assert_eq!(rows[0].1, "pedido", "job_type must match the seeded value");
}

// ── RES-01: Retry poll (D-06) ─────────────────────────────────────────────────

/// Assert the D-06 poll query returns only due rows (next_retry_at <= now) and
/// returns them ordered oldest-first.
///
/// Seeds three retry_queue rows:
///   - past-old: next_retry_at 2 minutes ago   → due, returned first
///   - past-new: next_retry_at 30 seconds ago  → due, returned second
///   - future:   next_retry_at 5 minutes from now → NOT due, not returned
#[test]
fn retry_poll_selects_due_rows_oldest_first() {
    let conn = make_test_conn();

    conn.execute_batch(
        "INSERT INTO printed_jobs (job_id, job_type, status)
             VALUES ('job-old', 'pedido', 'printing');
         INSERT INTO printed_jobs (job_id, job_type, status)
             VALUES ('job-new', 'despacho', 'printing');
         INSERT INTO printed_jobs (job_id, job_type, status)
             VALUES ('job-future', 'pedido', 'pending');",
    )
    .expect("seed printed_jobs");

    conn.execute_batch(
        "INSERT INTO retry_queue (job_id, job_type, attempt_count, next_retry_at, created_at)
             VALUES ('job-old',    'pedido',   1, datetime('now','-2 minutes'),  datetime('now','-3 minutes'));
         INSERT INTO retry_queue (job_id, job_type, attempt_count, next_retry_at, created_at)
             VALUES ('job-new',    'despacho', 1, datetime('now','-30 seconds'), datetime('now','-60 seconds'));
         INSERT INTO retry_queue (job_id, job_type, attempt_count, next_retry_at, created_at)
             VALUES ('job-future', 'pedido',   1, datetime('now','+5 minutes'),  datetime('now'));",
    )
    .expect("seed retry_queue");

    // D-06 poll query
    let mut stmt = conn
        .prepare(
            "SELECT job_id, job_type, escpos_bytes, attempt_count
             FROM retry_queue
             WHERE next_retry_at <= datetime('now')
             ORDER BY next_retry_at ASC
             LIMIT 10",
        )
        .expect("prepare D-06 query");

    let rows: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();

    assert_eq!(rows.len(), 2, "only 2 due rows must be returned (future row excluded)");
    assert_eq!(rows[0], "job-old", "oldest-first: job-old must come first");
    assert_eq!(rows[1], "job-new", "oldest-first: job-new must come second");
}

// ── RES-01: retry_queue INSERT (D-12) ────────────────────────────────────────

/// Assert the D-12 INSERT into retry_queue:
///   1. Stores `escpos_bytes` as a BLOB that round-trips correctly.
///   2. `INSERT OR IGNORE` deduplicates on re-entry (attempt_count unchanged).
///
/// RES-01: bytes must survive the round-trip so the retry task can re-print them.
#[test]
fn retry_queue_insert_stores_blob_bytes() {
    let conn = make_test_conn();

    // Seed the parent printed_jobs row (retry_queue rows reference printed_jobs rows
    // in the production schema — but no FK constraint in test schema, just good practice).
    conn.execute(
        "INSERT INTO printed_jobs (job_id, job_type, status) VALUES (?1, ?2, 'printing')",
        rusqlite::params!["job-blob", "pedido"],
    )
    .expect("seed printed_jobs");

    // ESC/POS bytes: ESC @ (reset) followed by ASCII 'A'
    let bytes: Vec<u8> = vec![0x1b, 0x40, 0x41];

    // D-12 INSERT: attempt_count=1, next_retry_at = now + 30s
    let rows_changed = conn
        .execute(
            "INSERT OR IGNORE INTO retry_queue
                 (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
             VALUES
                 (?1, ?2, ?3, 1, datetime('now','+30 seconds'), ?4, datetime('now'))",
            rusqlite::params!["job-blob", "pedido", bytes.as_slice(), "print error"],
        )
        .expect("D-12 INSERT");
    assert_eq!(rows_changed, 1, "first INSERT must affect 1 row");

    // Read back and verify BLOB round-trip
    let stored: Vec<u8> = conn
        .query_row(
            "SELECT escpos_bytes FROM retry_queue WHERE job_id = ?1",
            rusqlite::params!["job-blob"],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .expect("SELECT escpos_bytes");
    assert_eq!(stored, bytes, "ESC/POS bytes must round-trip via BLOB column");

    // Second INSERT OR IGNORE for the same job_id must be a no-op
    let rows_changed_2 = conn
        .execute(
            "INSERT OR IGNORE INTO retry_queue
                 (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
             VALUES
                 (?1, ?2, ?3, 99, datetime('now','+60 seconds'), ?4, datetime('now'))",
            rusqlite::params!["job-blob", "pedido", bytes.as_slice(), "second attempt"],
        )
        .expect("D-12 second INSERT OR IGNORE");
    assert_eq!(rows_changed_2, 0, "INSERT OR IGNORE must be a no-op on duplicate job_id");

    // Verify attempt_count was NOT updated by the second insert (dedup fence)
    let attempt_count: i64 = conn
        .query_row(
            "SELECT attempt_count FROM retry_queue WHERE job_id = ?1",
            rusqlite::params!["job-blob"],
            |row| row.get(0),
        )
        .expect("SELECT attempt_count");
    assert_eq!(attempt_count, 1, "attempt_count must remain 1 after INSERT OR IGNORE no-op (dedup fence)");
}

// ── RES-02: Retry exhaustion (Wave 2 placeholder) ────────────────────────────

/// Documents the retry exhaustion invariant (RES-02):
///
/// After a failed print when `attempt_count >= 3`, the retry task must:
///   1. DELETE the row from `retry_queue`
///   2. UPDATE `printed_jobs SET status='failed', failed_at=datetime('now')`
///   3. Send `HealthState::Problem` (red tray icon)
///   4. Show a Windows toast notification (D-07)
///
/// This test will be activated in Wave 2 when `run_retry_task` is implemented.
#[test]
#[ignore = "Wave 2: run_retry_task not yet implemented"]
fn retry_exhaustion_marks_failed() {
    // Wave 2 will call run_retry_task with a mock printer that always fails,
    // seed retry_queue with attempt_count=3, and assert:
    //   - retry_queue row is DELETEd after the exhaustion attempt
    //   - printed_jobs.status = 'failed' with failed_at set
    //   - health_state Problem callback was invoked
    todo!(
        "Wave 2: implement run_retry_task in src/retry_task.rs (D-03/D-06), \
         then remove #[ignore] and wire up the mock printer + health state spy"
    )
}

// ── RES-01/02/04: Retry task smoke (Wave 2 placeholder) ─────────────────────

/// Smoke test for `brevly_print::retry_task::run_retry_task`.
///
/// This test documents the module entry point and will be activated in Wave 2.
/// The function signature per D-03:
/// ```
/// pub async fn run_retry_task(
///     db_path: PathBuf,
///     agent_token: String,
///     base_url: String,
///     http: reqwest::Client,
///     printer: Box<dyn Printer + Send>,
///     send_health: impl Fn(HealthState) + Send + 'static,
/// )
/// ```
#[test]
#[ignore = "Wave 2: run_retry_task not yet implemented"]
fn retry_task_smoke() {
    // Wave 2: verify run_retry_task spawns without panic, processes a due retry_queue
    // row, and calls ack_job on success.  Use a mock printer and in-memory DB.
    todo!(
        "Wave 2: implement src/retry_task.rs with pub async fn run_retry_task(...) \
         per D-03 in 06-CONTEXT.md, then remove #[ignore]"
    )
}
