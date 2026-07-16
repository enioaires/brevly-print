//! Integration tests for Phase 6 resilience: RES-01, RES-02, RES-04.
//!
//! The SQL-invariant tests (crash recovery, poll query, blob round-trip, exhaustion
//! SQL) were the original Wave 0 scaffolds. The two end-to-end tests
//! (`retry_exhaustion_marks_failed` and `retry_task_smoke`) were added as RED stubs
//! in Wave 0 and are now activated (W-01 gap closure) to drive the REAL poll-loop
//! iteration code (`brevly_print::retry_task::process_due_retries_once`).
//!
//! Portable: runs on Linux and Windows (no Windows-API dependency in this file).

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpListener;

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

// ── End-to-end helpers ────────────────────────────────────────────────────────

/// Open an in-memory SQLite connection and run the REAL production migrations
/// (including the v2 schema with the `'printing'` CHECK and retry_queue FK).
///
/// Replaces the hand-rolled `make_test_conn` for tests that drive production code,
/// so the schema cannot drift from what `process_due_retries_once` expects.
fn make_migrated_conn() -> Connection {
    let mut conn = Connection::open_in_memory().expect("in-memory DB");
    brevly_print::config_store::migrate(&mut conn).expect("run migrations");
    conn
}

/// Spawn a local HTTP stub that accepts ONE request and returns `status`.
/// Returns the base URL (e.g. `"http://127.0.0.1:PORT"`).
async fn spawn_ack_stub(status: u16) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
    let port = listener.local_addr().unwrap().port();

    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
        socket.write_all(response.as_bytes()).await.expect("write");
        socket.shutdown().await.ok();
    });

    format!("http://127.0.0.1:{port}")
}

/// A mock printer that always fails. Implements `Printer + Send + Sync`.
struct AlwaysFailPrinter;

impl brevly_print::printer::Printer for AlwaysFailPrinter {
    fn print_raw(&self, _bytes: &[u8]) -> Result<(), brevly_print::printer::PrinterError> {
        Err(brevly_print::printer::PrinterError::PrintFailed(
            "mock printer always fails".to_string(),
        ))
    }
}

/// A mock printer that always succeeds.
struct AlwaysOkPrinter;

impl brevly_print::printer::Printer for AlwaysOkPrinter {
    fn print_raw(&self, _bytes: &[u8]) -> Result<(), brevly_print::printer::PrinterError> {
        Ok(())
    }
}

// ── RES-02: End-to-end retry exhaustion ──────────────────────────────────────

/// End-to-end test: `process_due_retries_once` with an always-failing printer
/// and a due row at `attempt_count=3` (exhaustion threshold).
///
/// After one call to the real poll-loop iteration body, asserts:
///   1. `retry_queue` row is DELETEd.
///   2. `printed_jobs.status = 'failed'` with `failed_at` set.
///   3. `send_health(HealthState::Problem)` was called exactly once.
///
/// This is the W-01 gap closure: the prior in-module `retry_exhaustion_marks_failed`
/// unit test re-implemented the SQL inline; this test calls the REAL production
/// function including the `send_health` side-effect.
#[tokio::test]
async fn retry_exhaustion_marks_failed() {
    let conn = make_migrated_conn();

    // Seed printed_jobs first (retry_queue has a FK reference).
    conn.execute(
        "INSERT INTO printed_jobs (job_id, job_type, status) VALUES ('job-ex2', 'pedido', 'pending')",
        [],
    )
    .expect("seed printed_jobs");

    // Seed retry_queue at attempt_count=3 (exhaustion threshold), due immediately.
    conn.execute(
        "INSERT INTO retry_queue
             (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
         VALUES ('job-ex2', 'pedido', X'1b4041', 3, datetime('now','-1 second'), 'prev error', datetime('now'))",
        [],
    )
    .expect("seed retry_queue");

    // Health spy: records every HealthState delivered by the production loop.
    let health_calls: Arc<Mutex<Vec<brevly_print::health_state::HealthState>>> =
        Arc::new(Mutex::new(Vec::new()));
    let health_spy = {
        let calls = Arc::clone(&health_calls);
        move |state: brevly_print::health_state::HealthState| {
            calls.lock().unwrap().push(state);
        }
    };

    let printer = AlwaysFailPrinter;
    let http = reqwest::Client::new();
    // Use a non-existent base_url — exhaustion path does NOT call ack_job, so no HTTP hit.
    let (conn, rows_processed) = brevly_print::retry_task::process_due_retries_once(
        conn,
        "test-token",
        "http://127.0.0.1:1", // unreachable — ack not called on exhaustion
        &http,
        &printer,
        &health_spy,
    )
    .await;

    // ── Assertions ──────────────────────────────────────────────────────────

    assert_eq!(rows_processed, 1, "one due row must have been processed");

    // retry_queue must be DELETEd.
    let rq_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM retry_queue WHERE job_id='job-ex2'",
            [],
            |r| r.get(0),
        )
        .expect("count retry_queue");
    assert_eq!(rq_count, 0, "retry_queue row must be DELETEd after exhaustion");

    // printed_jobs.status must be 'failed'.
    let status: String = conn
        .query_row(
            "SELECT status FROM printed_jobs WHERE job_id='job-ex2'",
            [],
            |r| r.get(0),
        )
        .expect("SELECT status");
    assert_eq!(status, "failed", "printed_jobs.status must be 'failed'");

    // failed_at must be set.
    let failed_at: Option<String> = conn
        .query_row(
            "SELECT failed_at FROM printed_jobs WHERE job_id='job-ex2'",
            [],
            |r| r.get(0),
        )
        .expect("SELECT failed_at");
    assert!(failed_at.is_some(), "failed_at must be set after exhaustion");

    // send_health(Problem) must have been called exactly once.
    let calls = health_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "send_health must be called once (exhaustion)");
    assert!(
        matches!(calls[0], brevly_print::health_state::HealthState::Problem),
        "send_health must receive HealthState::Problem on exhaustion; got: {:?}",
        calls[0]
    );
}

// ── RES-01/02: Retry task smoke ───────────────────────────────────────────────

/// End-to-end smoke test: `process_due_retries_once` with an always-succeeding
/// printer and a due row at `attempt_count=1` (first retry).
///
/// Uses a local HTTP stub that returns 200 for the `ack_job` POST.
///
/// After one call to the real poll-loop iteration body, asserts:
///   1. `printed_jobs.status = 'printed'` with `printed_at` set.
///   2. `retry_queue` row is DELETEd.
///   3. `send_health(HealthState::Connected)` was called once.
///
/// This is the W-01 gap closure: the production loop's success path (print +
/// UPDATE 'printed' + ack + DELETE) is executed end-to-end including the HTTP
/// ack call and the `send_health` side-effect.
#[tokio::test]
async fn retry_task_smoke() {
    let conn = make_migrated_conn();

    // Seed printed_jobs first (FK constraint).
    conn.execute(
        "INSERT INTO printed_jobs (job_id, job_type, status) VALUES ('job-smoke', 'pedido', 'pending')",
        [],
    )
    .expect("seed printed_jobs");

    // Seed retry_queue — due immediately, attempt_count=1.
    conn.execute(
        "INSERT INTO retry_queue
             (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
         VALUES ('job-smoke', 'pedido', X'1b4041', 1, datetime('now','-1 second'), 'first fail', datetime('now'))",
        [],
    )
    .expect("seed retry_queue");

    // HTTP stub: return 200 for the ack POST.
    let base_url = spawn_ack_stub(200).await;

    // Health spy.
    let health_calls: Arc<Mutex<Vec<brevly_print::health_state::HealthState>>> =
        Arc::new(Mutex::new(Vec::new()));
    let health_spy = {
        let calls = Arc::clone(&health_calls);
        move |state: brevly_print::health_state::HealthState| {
            calls.lock().unwrap().push(state);
        }
    };

    let printer = AlwaysOkPrinter;
    let http = reqwest::Client::new();
    let (conn, rows_processed) = brevly_print::retry_task::process_due_retries_once(
        conn,
        "test-token",
        &base_url,
        &http,
        &printer,
        &health_spy,
    )
    .await;

    // ── Assertions ──────────────────────────────────────────────────────────

    assert_eq!(rows_processed, 1, "one due row must have been processed");

    // printed_jobs.status must be 'printed'.
    let status: String = conn
        .query_row(
            "SELECT status FROM printed_jobs WHERE job_id='job-smoke'",
            [],
            |r| r.get(0),
        )
        .expect("SELECT status");
    assert_eq!(status, "printed", "printed_jobs.status must be 'printed' after success");

    // printed_at must be set.
    let printed_at: Option<String> = conn
        .query_row(
            "SELECT printed_at FROM printed_jobs WHERE job_id='job-smoke'",
            [],
            |r| r.get(0),
        )
        .expect("SELECT printed_at");
    assert!(printed_at.is_some(), "printed_at must be set after successful retry");

    // retry_queue must be DELETEd.
    let rq_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM retry_queue WHERE job_id='job-smoke'",
            [],
            |r| r.get(0),
        )
        .expect("count retry_queue");
    assert_eq!(rq_count, 0, "retry_queue row must be DELETEd after success");

    // send_health(Connected) must have been called once.
    let calls = health_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "send_health must be called once (success)");
    assert!(
        matches!(calls[0], brevly_print::health_state::HealthState::Connected),
        "send_health must receive HealthState::Connected on success; got: {:?}",
        calls[0]
    );
}
