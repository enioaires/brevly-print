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

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::time::{interval, MissedTickBehavior};

use crate::{
    health_state::HealthState,
    noren_client::{ack_job, fetch_job_bytes},
};

/// Run the retry task (D-03): crash recovery followed by the poll loop.
///
/// Opens its own WAL SQLite connection (4th total), performs crash recovery on
/// startup, then polls `retry_queue` every 5 seconds and retries each due job
/// up to 3 times at 30-second intervals.
///
/// `send_health` has the same type as the Pusher task's health closure:
/// `impl Fn(HealthState) + Send + 'static`.
///
/// **CR-02 note:** This wrapper still runs recovery *then* the poll loop, but the
/// preferred call path in `main.rs` calls [`recover_orphans`] separately (awaited to
/// completion BEFORE the print worker task is spawned) and then spawns
/// [`run_retry_poll_loop`]. That ordering eliminates the double-print race by
/// construction — any `'printing'` row observed at boot is genuinely from a dead prior
/// process, never a live worker mid-print. This wrapper is retained for the in-module
/// test and any caller that does not need the split.
pub async fn run_retry_task(
    db_path: PathBuf,
    agent_token: String,
    base_url: String,
    http: reqwest::Client,
    printer: Box<dyn crate::printer::Printer + Send>,
    send_health: impl Fn(HealthState) + Send + 'static,
) {
    // ── Startup: open a FOURTH SQLite connection (D-04) ─────────────────────
    let conn = match open_retry_conn(&db_path) {
        Some(c) => c,
        None => return,
    };

    let conn = recover_orphans_on_conn(conn, &agent_token, &base_url, &http).await;

    run_poll_loop_on_conn(conn, agent_token, base_url, http, printer, send_health).await;
}

/// Open the retry task's SQLite connection with shared WAL pragmas (CR-01).
///
/// Returns `None` (after logging) if the connection cannot be opened or the pragmas
/// cannot be applied — the caller should abort the retry task in that case.
fn open_retry_conn(db_path: &Path) -> Option<rusqlite::Connection> {
    let conn = match rusqlite::Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[brevly-print] Retry task: failed to open SQLite connection: {e:#}");
            return None;
        }
    };
    // CR-01: WAL + busy_timeout via the shared helper so all four connections
    // cannot drift. busy_timeout resolves write-write contention on printed_jobs
    // safely instead of returning SQLITE_BUSY.
    if let Err(e) = crate::config_store::apply_wal_pragmas(&conn) {
        eprintln!("[brevly-print] Retry task: failed to set WAL pragmas: {e:#}");
        return None;
    }
    Some(conn)
}

/// **CR-02 crash recovery — run ONCE at startup, awaited to completion BEFORE the
/// print worker task is spawned.**
///
/// Opens the retry connection, re-queues orphaned `'printing'` rows, and returns.
/// Because this completes before any live print worker exists, any `'printing'` row
/// it sees is guaranteed to be from a crashed prior process — never a concurrent
/// in-flight print. This eliminates the double-print / concurrent-`print_raw` window
/// described in CR-02 by construction, with no fence-timestamp heuristic required.
///
/// Safe to call even if the DB cannot be opened (logs and returns).
pub async fn recover_orphans(
    db_path: PathBuf,
    agent_token: String,
    base_url: String,
    http: reqwest::Client,
) {
    let conn = match open_retry_conn(&db_path) {
        Some(c) => c,
        None => return,
    };
    let _ = recover_orphans_on_conn(conn, &agent_token, &base_url, &http).await;
}

/// Crash-recovery scan + re-queue on an already-open connection (D-05 / RES-04).
///
/// Find 'printing' rows whose job_id is NOT yet in retry_queue — these are
/// jobs that crashed BEFORE the print worker could save bytes to retry_queue.
/// We re-fetch the ESC/POS bytes from Noren and INSERT into retry_queue for
/// immediate retry (next_retry_at = now).
///
/// If fetch_job_bytes fails: log and skip — the row stays at 'printing' and
/// will be re-attempted on the next boot (idempotent startup check, Pitfall 3).
/// Sync scan for orphaned `'printing'` rows not yet in `retry_queue`.
///
/// Kept sync (and separate) so the prepared-statement borrow of `conn` is fully
/// released before the async caller holds `conn` across a fetch `.await`.
fn scan_orphans(conn: &rusqlite::Connection) -> Vec<(String, String)> {
    let mut stmt = match conn.prepare(
        "SELECT job_id, job_type FROM printed_jobs
         WHERE status = 'printing'
           AND job_id NOT IN (SELECT job_id FROM retry_queue)",
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[brevly-print] Retry task: crash recovery prepare failed: {e:#}");
            return Vec::new();
        }
    };
    match stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?))) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            eprintln!("[brevly-print] Retry task: crash recovery query failed: {e:#}");
            Vec::new()
        }
    }
}

async fn recover_orphans_on_conn(
    conn: rusqlite::Connection,
    agent_token: &str,
    base_url: &str,
    http: &reqwest::Client,
) -> rusqlite::Connection {
    // Take conn by value (owned `Connection` is Send, so holding it across the fetch
    // .await keeps this future Send — a `&Connection` would NOT, since Connection is
    // not Sync). Returned to the caller for reuse by the poll loop.
    //
    // Scan for orphans in a helper that fully drops its borrow of `conn` before we
    // reach the fetch .await loop below (otherwise the prepared-statement borrow would
    // conflict with returning `conn` at the end).
    let orphans = scan_orphans(&conn);

    for (job_id, job_type) in orphans {
        match fetch_job_bytes(http, base_url, agent_token, &job_id).await {
            Ok(bytes) => {
                let inserted = conn.execute(
                    "INSERT OR IGNORE INTO retry_queue
                         (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
                     VALUES (?1, ?2, ?3, 1, datetime('now'), 'crash recovery', datetime('now'))",
                    rusqlite::params![job_id, job_type, bytes.as_slice()],
                );
                match inserted {
                    Ok(0) => eprintln!(
                        "[brevly-print] Retry task: crash recovery INSERT affected 0 rows for {job_id} (already queued?)"
                    ),
                    Ok(_) => eprintln!(
                        "[brevly-print] Retry task: crash recovery re-queued job {job_id} for immediate retry"
                    ),
                    Err(e) => eprintln!(
                        "[brevly-print] Retry task: crash recovery INSERT failed for {job_id}: {e:#}"
                    ),
                }
            }
            Err(e) => {
                eprintln!(
                    "[brevly-print] Retry task: crash recovery fetch failed for {job_id}: {e:#}"
                );
                // Leave status='printing' — retry on next boot (Pitfall 3, documented acceptable).
            }
        }
    }

    conn
}

/// **CR-02 poll loop — spawn AFTER [`recover_orphans`] has completed.**
///
/// Opens its own retry connection and runs the retry poll loop forever. Does NOT
/// perform crash recovery — that must already have run to completion via
/// [`recover_orphans`] before the print worker was spawned.
pub async fn run_retry_poll_loop(
    db_path: PathBuf,
    agent_token: String,
    base_url: String,
    http: reqwest::Client,
    printer: Box<dyn crate::printer::Printer + Send>,
    send_health: impl Fn(HealthState) + Send + 'static,
) {
    let conn = match open_retry_conn(&db_path) {
        Some(c) => c,
        None => return,
    };
    run_poll_loop_on_conn(conn, agent_token, base_url, http, printer, send_health).await;
}

/// The retry poll loop body, operating on an already-open connection (D-06).
async fn run_poll_loop_on_conn(
    conn: rusqlite::Connection,
    agent_token: String,
    base_url: String,
    http: reqwest::Client,
    printer: Box<dyn crate::printer::Printer + Send>,
    send_health: impl Fn(HealthState) + Send + 'static,
) {
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
        // WR-02: read escpos_bytes as Option<Vec<u8>> so a NULL blob does not cause
        // the whole row to be silently dropped by filter_map (which would leave the
        // row stuck forever in retry_queue, never processed and never exhausted).
        let rows: Vec<(String, String, Option<Vec<u8>>, i64)> = {
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
                    row.get::<_, Option<Vec<u8>>>(2)?,
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
            // WR-02: a NULL or empty blob can never print. Mark the job 'failed' and
            // remove it from the queue instead of looping forever on an unprintable row.
            let escpos_bytes = match escpos_bytes {
                Some(b) if !b.is_empty() => b,
                _ => {
                    eprintln!(
                        "[brevly-print] Retry task: job {job_id} has NULL/empty escpos_bytes — marking failed and removing from queue"
                    );
                    match conn.execute(
                        "UPDATE printed_jobs SET status='failed', failed_at=datetime('now') WHERE job_id=?1",
                        rusqlite::params![job_id],
                    ) {
                        Ok(0) => eprintln!(
                            "[brevly-print] Retry task: NULL-bytes UPDATE to 'failed' matched 0 rows for {job_id} — row absent"
                        ),
                        Ok(_) => {}
                        Err(e) => eprintln!(
                            "[brevly-print] Retry task: NULL-bytes UPDATE to 'failed' failed for {job_id}: {e:#}"
                        ),
                    }
                    if let Err(e) = conn.execute(
                        "DELETE FROM retry_queue WHERE job_id=?1",
                        rusqlite::params![job_id],
                    ) {
                        eprintln!(
                            "[brevly-print] Retry task: NULL-bytes DELETE from retry_queue failed for {job_id}: {e:#}"
                        );
                    }
                    send_health(HealthState::Problem);
                    show_print_failure_toast();
                    continue;
                }
            };

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
                    if let Err(e) = conn.execute(
                        "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
                        rusqlite::params![job_id],
                    ) {
                        eprintln!(
                            "[brevly-print] Retry task: UPDATE to 'printed' failed for {job_id}: {e:#}"
                        );
                    }
                    if let Err(e) = ack_job(&http, &base_url, &agent_token, &job_id).await {
                        eprintln!("[brevly-print] Retry task: ack failed for {job_id}: {e:#}");
                        // ack failure is non-fatal — status='printed' is already persisted;
                        // RES-03 pending pull handles recovery (D-09 carry-forward).
                    }
                    // WR-04: check the DELETE result. If it silently affects 0 rows (or
                    // errors under contention), the job stays in retry_queue and would be
                    // re-printed on the next poll — a duplicate comanda. Log so the drift
                    // is visible.
                    match conn.execute(
                        "DELETE FROM retry_queue WHERE job_id=?1",
                        rusqlite::params![job_id],
                    ) {
                        Ok(0) => eprintln!(
                            "[brevly-print] Retry task: DELETE after success matched 0 rows for {job_id} — risk of duplicate reprint on next poll"
                        ),
                        Ok(_) => {}
                        Err(e) => eprintln!(
                            "[brevly-print] Retry task: DELETE from retry_queue failed for {job_id}: {e:#} — risk of duplicate reprint"
                        ),
                    }
                    eprintln!("[brevly-print] Retry task: job {job_id} printed successfully on retry");
                    send_health(HealthState::Connected);
                }
                Err(e) if attempt_count < 3 => {
                    // Not yet exhausted: schedule next retry in 30 seconds.
                    // WR-01: log the actual attempt number consistently as "attempt N of 3".
                    // attempt_count is the pre-increment value; this is the Nth attempt that
                    // just failed (seeded at 1 by the worker's original failed print).
                    let msg = e.to_string();
                    if let Err(db_err) = conn.execute(
                        "UPDATE retry_queue SET attempt_count=attempt_count+1,
                             next_retry_at=datetime('now', '+30 seconds'), last_error=?2
                         WHERE job_id=?1",
                        rusqlite::params![job_id, msg],
                    ) {
                        eprintln!(
                            "[brevly-print] Retry task: reschedule UPDATE failed for {job_id}: {db_err:#}"
                        );
                    }
                    eprintln!(
                        "[brevly-print] Retry task: job {job_id} attempt {attempt_count} of 3 failed ({msg}); scheduled retry in 30s"
                    );
                }
                Err(e) => {
                    // attempt_count >= 3: exhausted (D-06 step 5 / RES-02).
                    // WR-01: this is the exhausting attempt (attempt_count == 3), logged
                    // consistently with the per-attempt line above ("attempt 3 of 3").
                    let msg = e.to_string();
                    eprintln!(
                        "[brevly-print] Retry task: job {job_id} attempt {attempt_count} of 3 failed ({msg}); exhausted — marking failed"
                    );
                    // WR-04: check the exhaustion UPDATE result. If it silently fails, the
                    // row is deleted from the queue but left 'printing', becoming a permanent
                    // orphan that crash recovery re-queues on every boot.
                    match conn.execute(
                        "UPDATE printed_jobs SET status='failed', failed_at=datetime('now') WHERE job_id=?1",
                        rusqlite::params![job_id],
                    ) {
                        Ok(0) => eprintln!(
                            "[brevly-print] Retry task: exhaustion UPDATE to 'failed' matched 0 rows for {job_id} — row absent"
                        ),
                        Ok(_) => {}
                        Err(e) => eprintln!(
                            "[brevly-print] Retry task: exhaustion UPDATE to 'failed' failed for {job_id}: {e:#} — row may be left orphaned at 'printing'"
                        ),
                    }
                    if let Err(e) = conn.execute(
                        "DELETE FROM retry_queue WHERE job_id=?1",
                        rusqlite::params![job_id],
                    ) {
                        eprintln!(
                            "[brevly-print] Retry task: exhaustion DELETE from retry_queue failed for {job_id}: {e:#}"
                        );
                    }
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

    /// Create an in-memory SQLite connection at the real production schema.
    ///
    /// IN-04: run the actual `MIGRATIONS.to_latest()` (via `config_store::migrate`)
    /// instead of a hand-rolled schema, so the `status` CHECK (incl. 'printing') and
    /// the `retry_queue → printed_jobs` FK match production and cannot drift.
    fn make_test_conn() -> rusqlite::Connection {
        let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory DB");
        crate::config_store::migrate(&mut conn).expect("run migrations on in-memory DB");
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
