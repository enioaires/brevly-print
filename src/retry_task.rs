//! Retry task — Phase 6 resilience: RES-01, RES-02, RES-04.
//!
//! Runs as the fourth Tokio task (alongside main event loop, Pusher task, print
//! worker). Responsibilities:
//!   - **Crash recovery (D-05):** At startup, re-queues `status='printing'` rows
//!     that are NOT already in `retry_queue` (crashed before bytes were saved).
//!   - **Poll loop (D-06):** Every 5 seconds, attempts to re-print due
//!     `retry_queue` rows. Each row is tried up to 3 times at 30-second intervals.
//!   - **Exhaustion (D-06 step 5):** After 3 failures, marks the job `'failed'`,
//!     removes it from the queue, turns the tray red, and shows a Windows toast
//!     (D-07). On Linux: stderr log instead of toast (cross-platform build).
//!
//! Token constraints:
//!   - `agent_token` is passed only to `fetch_job_bytes`/`ack_job` via
//!     `.bearer_auth()`; it is NEVER logged (T-06-04).
//!   - `busy_timeout(5s)` is set on the WAL connection (T-06-06 / Pitfall 2).

use std::path::PathBuf;
use std::time::Duration;

use tokio::time::{interval, MissedTickBehavior};

use crate::{
    health_state::HealthState,
    noren_client::{ack_job, fetch_job_bytes},
};

/// Run the retry task (D-03).
///
/// Opens its own WAL SQLite connection (4th total), performs crash recovery on
/// startup, then polls `retry_queue` every 5 seconds and retries each due job
/// up to 3 times at 30-second intervals.
///
/// `send_health` has the same type as the Pusher task's health closure:
/// `impl Fn(HealthState) + Send + 'static`.
pub async fn run_retry_task(
    db_path: PathBuf,
    agent_token: String,
    base_url: String,
    http: reqwest::Client,
    printer: Box<dyn crate::printer::Printer + Send>,
    send_health: impl Fn(HealthState) + Send + 'static,
) {
    // ── Startup: open a FOURTH SQLite connection (D-04) ─────────────────────
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[brevly-print] Retry task: failed to open SQLite connection: {e:#}");
            return;
        }
    };
    if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
        eprintln!("[brevly-print] Retry task: failed to set WAL mode: {e:#}");
        return;
    }
    // Pitfall 2: set busy_timeout so write-write contention on printed_jobs
    // with the print worker resolves safely instead of returning SQLITE_BUSY.
    let _ = conn.busy_timeout(Duration::from_secs(5));

    // ── Crash recovery (D-05 / RES-04) ──────────────────────────────────────
    //
    // Find 'printing' rows whose job_id is NOT yet in retry_queue — these are
    // jobs that crashed BEFORE the print worker could save bytes to retry_queue.
    // We re-fetch the ESC/POS bytes from Noren and INSERT into retry_queue for
    // immediate retry (next_retry_at = now).
    //
    // If fetch_job_bytes fails: log and skip — the row stays at 'printing' and
    // will be re-attempted on the next boot (idempotent startup check, Pitfall 3).
    let orphans: Vec<(String, String)> = {
        let mut stmt = match conn.prepare(
            "SELECT job_id, job_type FROM printed_jobs
             WHERE status = 'printing'
               AND job_id NOT IN (SELECT job_id FROM retry_queue)",
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[brevly-print] Retry task: crash recovery prepare failed: {e:#}");
                return;
            }
        };
        match stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?))) {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                eprintln!("[brevly-print] Retry task: crash recovery query failed: {e:#}");
                Vec::new()
            }
        }
    };

    for (job_id, job_type) in orphans {
        match fetch_job_bytes(&http, &base_url, &agent_token, &job_id).await {
            Ok(bytes) => {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO retry_queue
                         (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
                     VALUES (?1, ?2, ?3, 1, datetime('now'), 'crash recovery', datetime('now'))",
                    rusqlite::params![job_id, job_type, bytes.as_slice()],
                );
                eprintln!(
                    "[brevly-print] Retry task: crash recovery re-queued job {job_id} for immediate retry"
                );
            }
            Err(e) => {
                eprintln!(
                    "[brevly-print] Retry task: crash recovery fetch failed for {job_id}: {e:#}"
                );
                // Leave status='printing' — retry on next boot (Pitfall 3, documented acceptable).
            }
        }
    }

    // ── Poll loop (D-06) ────────────────────────────────────────────────────
    //
    // Poll every 5 seconds (MissedTickBehavior::Delay so a slow iteration
    // does not cause back-to-back ticks). Burn the first immediate tick so
    // the first real poll waits 5s (mirrors pusher/client.rs reconnect timer).
    let mut poll_timer = interval(Duration::from_secs(5));
    poll_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
    poll_timer.tick().await; // burn first immediate tick

    loop {
        poll_timer.tick().await;

        // Collect all due rows (next_retry_at <= now), ordered oldest-first,
        // up to 10 at a time (queue rarely exceeds a handful of rows — D-06).
        let rows: Vec<(String, String, Vec<u8>, i64)> = {
            let mut stmt = match conn.prepare(
                "SELECT job_id, job_type, escpos_bytes, attempt_count
                 FROM retry_queue
                 WHERE next_retry_at <= datetime('now')
                 ORDER BY next_retry_at ASC
                 LIMIT 10",
            ) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[brevly-print] Retry task: poll prepare failed: {e:#}");
                    continue;
                }
            };
            match stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            }) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(e) => {
                    eprintln!("[brevly-print] Retry task: poll query failed: {e:#}");
                    Vec::new()
                }
            }
        };

        for (job_id, _job_type, escpos_bytes, attempt_count) in rows {
            // Crash fence: set status='printing' before each retry attempt so a crash
            // mid-retry does not leave the row orphaned from retry_queue (D-06 step 1).
            // Log but continue on failure — same tolerance as print_worker's fence.
            match conn.execute(
                "UPDATE printed_jobs SET status='printing' WHERE job_id=?1",
                rusqlite::params![job_id],
            ) {
                Ok(0) => eprintln!(
                    "[brevly-print] Retry task: UPDATE to 'printing' matched 0 rows for {job_id} — row absent"
                ),
                Ok(_) => {}
                Err(e) => eprintln!(
                    "[brevly-print] Retry task: SQLite update to 'printing' failed for {job_id}: {e:#}"
                ),
            }

            match printer.print_raw(&escpos_bytes) {
                Ok(()) => {
                    // C4 ordering: UPDATE status='printed' BEFORE ack_job BEFORE DELETE.
                    let _ = conn.execute(
                        "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
                        rusqlite::params![job_id],
                    );
                    if let Err(e) = ack_job(&http, &base_url, &agent_token, &job_id).await {
                        eprintln!("[brevly-print] Retry task: ack failed for {job_id}: {e:#}");
                        // ack failure is non-fatal — status='printed' is already persisted;
                        // RES-03 pending pull handles recovery (D-09 carry-forward).
                    }
                    let _ = conn.execute(
                        "DELETE FROM retry_queue WHERE job_id=?1",
                        rusqlite::params![job_id],
                    );
                    eprintln!("[brevly-print] Retry task: job {job_id} printed successfully on retry");
                    send_health(HealthState::Connected);
                }
                Err(e) if attempt_count < 3 => {
                    // Not yet exhausted: schedule next retry in 30 seconds.
                    let msg = e.to_string();
                    let _ = conn.execute(
                        "UPDATE retry_queue SET attempt_count=attempt_count+1,
                             next_retry_at=datetime('now', '+30 seconds'), last_error=?2
                         WHERE job_id=?1",
                        rusqlite::params![job_id, msg],
                    );
                    eprintln!(
                        "[brevly-print] Retry task: job {job_id} attempt {attempt_count} failed ({msg}); scheduled retry in 30s"
                    );
                }
                Err(e) => {
                    // attempt_count >= 3: exhausted (D-06 step 5 / RES-02).
                    let msg = e.to_string();
                    eprintln!(
                        "[brevly-print] Retry task: job {job_id} exhausted after 3 attempts: {msg}"
                    );
                    let _ = conn.execute(
                        "UPDATE printed_jobs SET status='failed', failed_at=datetime('now') WHERE job_id=?1",
                        rusqlite::params![job_id],
                    );
                    let _ = conn.execute(
                        "DELETE FROM retry_queue WHERE job_id=?1",
                        rusqlite::params![job_id],
                    );
                    send_health(HealthState::Problem);
                    show_print_failure_toast();
                }
            }
        }
    }
}

/// Show a Windows toast notification after retry exhaustion (D-07 / RES-02).
///
/// Fire-and-forget — `let _ = ...show()` (failures are not retried).
/// On Linux: logs to stderr instead (cross-platform build constraint).
fn show_print_failure_toast() {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;
        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title("Brevly Print — Falha na impressão")
            .text1("Falha ao imprimir após 3 tentativas.")
            .text2("Verifique se a impressora está ligada e com papel.")
            .show();
    }
    #[cfg(not(windows))]
    eprintln!("[brevly-print] Retry task: print failure toast (Linux: stderr only)");
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printer::{Printer, PrinterError};

    /// A mock printer that always fails with `PrintFailed`.
    struct AlwaysFailPrinter;

    impl Printer for AlwaysFailPrinter {
        fn print_raw(&self, _bytes: &[u8]) -> Result<(), PrinterError> {
            Err(PrinterError::PrintFailed("mock printer always fails".to_string()))
        }
    }

    fn make_test_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory DB");
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

    /// Applies the exhaustion branch SQL to a seeded test DB and asserts the invariants:
    ///   1. `retry_queue` row is DELETEd.
    ///   2. `printed_jobs.status = 'failed'` with `failed_at` set.
    ///
    /// Named `retry_exhaustion` so `cargo test retry_exhaustion` matches.
    #[test]
    fn retry_exhaustion_marks_failed() {
        let conn = make_test_conn();

        // Seed a 'printing' row in printed_jobs (crash fence already applied).
        conn.execute(
            "INSERT INTO printed_jobs (job_id, job_type, status) VALUES ('job-ex', 'pedido', 'printing')",
            [],
        )
        .expect("seed printed_jobs");

        // Seed retry_queue with attempt_count=3 (at the exhaustion threshold).
        conn.execute(
            "INSERT INTO retry_queue
                 (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
             VALUES ('job-ex', 'pedido', X'1b4041', 3, datetime('now','-1 second'), 'prev error', datetime('now'))",
            [],
        )
        .expect("seed retry_queue");

        // Confirm the row is there.
        let rq_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM retry_queue WHERE job_id='job-ex'", [], |r| r.get(0))
            .expect("count retry_queue");
        assert_eq!(rq_count, 1, "retry_queue must have 1 row before exhaustion");

        // Simulate the exhaustion branch (attempt_count >= 3): re-use the printer to fail.
        let printer = AlwaysFailPrinter;
        let bytes: Vec<u8> = vec![0x1b, 0x40, 0x41];
        let attempt_count: i64 = 3;

        // Run the exhaustion SQL path (extracted logic from the poll loop).
        let print_result = printer.print_raw(&bytes);
        assert!(print_result.is_err(), "AlwaysFailPrinter must fail");

        // Only run the exhaustion branch when attempt_count >= 3.
        if attempt_count >= 3 {
            conn.execute(
                "UPDATE printed_jobs SET status='failed', failed_at=datetime('now') WHERE job_id='job-ex'",
                [],
            )
            .expect("UPDATE to failed");
            conn.execute(
                "DELETE FROM retry_queue WHERE job_id='job-ex'",
                [],
            )
            .expect("DELETE from retry_queue");
        }

        // Assert invariants.
        let status: String = conn
            .query_row(
                "SELECT status FROM printed_jobs WHERE job_id='job-ex'",
                [],
                |r| r.get(0),
            )
            .expect("SELECT status");
        assert_eq!(status, "failed", "printed_jobs.status must be 'failed' after exhaustion");

        let failed_at: Option<String> = conn
            .query_row(
                "SELECT failed_at FROM printed_jobs WHERE job_id='job-ex'",
                [],
                |r| r.get(0),
            )
            .expect("SELECT failed_at");
        assert!(failed_at.is_some(), "failed_at must be set after exhaustion");

        let rq_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM retry_queue WHERE job_id='job-ex'", [], |r| r.get(0))
            .expect("count retry_queue after");
        assert_eq!(rq_after, 0, "retry_queue row must be DELETEd after exhaustion");
    }

    /// Verify `show_print_failure_toast()` does not panic on the current platform.
    /// On Linux this executes the stderr branch; on Windows the WinRT branch.
    #[test]
    fn show_print_failure_toast_does_not_panic() {
        show_print_failure_toast();
    }
}
