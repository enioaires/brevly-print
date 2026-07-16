//! Print worker — Phase 5 job pipeline.
//!
//! Runs as an independent Tokio task spawned in the Runtime block of `main.rs`.
//! Receives `PrintEvent` values from the Pusher task via an mpsc channel and
//! executes the full fetch → print → UPDATE → ack pipeline.
//!
//! Ordering constraint (C4): the SQLite `UPDATE status='printed'` MUST precede
//! every `ack_job` call, on every code path (disabled-type branch AND success branch).

use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::{
    config_store,
    noren_client::{ack_job, fetch_job_bytes},
    printer::{printer_from_entry, PrinterId},
    pusher::protocol::PrintEvent,
};

/// Run the print worker event loop.
///
/// Signature (D-01): receives `PrintEvent` values from the Pusher task, fetches
/// ESC/POS bytes from the Noren backend, prints via the configured printer, marks
/// the job as printed in SQLite, and acks the job back to Noren.
///
/// The task exits cleanly when the sender side of `rx` is dropped (channel closed).
pub async fn run_print_worker(
    mut rx: mpsc::Receiver<PrintEvent>,
    agent_token: String,
    base_url: String,
    db_path: PathBuf,
    http: reqwest::Client,
) {
    // ── Startup: open a SECOND SQLite connection (D-03) ─────────────────────────
    //
    // rusqlite::Connection is not Send; the main App.conn lives on the event-loop
    // thread, so this task must open its own connection (same pattern as pusher/client.rs).
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[brevly-print] Print worker: failed to open SQLite connection: {e:#}");
            return;
        }
    };
    // Enable WAL mode so concurrent writes from the main App connection are safe
    // (T-05-09 / Pitfall 5).
    if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
        eprintln!("[brevly-print] Print worker: failed to set WAL mode: {e:#}");
        return;
    }

    // ── Read enabled_types (fail-safe allow-all — D-03 / Pitfall 5 / T-05-06) ──
    //
    // If the key is missing or the JSON is malformed, unwrap_or_default() yields an
    // empty Vec, which the filter below treats as "allow all types".  This is intentional
    // — a misconfigured enabled_types must never silently drop jobs.
    let enabled_types: Vec<String> = config_store::get(&conn, "enabled_types")
        .unwrap_or(None)
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default();

    // ── Read printer configuration (D-03) ────────────────────────────────────────
    let printer_name = match config_store::get(&conn, "printer_name")
        .unwrap_or(None)
        .filter(|s| !s.is_empty())
    {
        Some(name) => name,
        None => {
            eprintln!("[brevly-print] Print worker: printer_name missing from ConfigStore");
            return;
        }
    };
    let printer_type = config_store::get(&conn, "printer_type")
        .unwrap_or(None)
        .unwrap_or_default();
    let printer_id = if printer_type == "serial" {
        PrinterId::Serial(printer_name)
    } else {
        PrinterId::Spooler(printer_name)
    };
    // Hold the Box<dyn Printer> for the lifetime of the task (C1: RAW datatype
    // is hardcoded in WindowsSpoolerPrinter::print_raw; do not add intermediate layers).
    let printer = printer_from_entry(&printer_id);

    // ── Event loop ───────────────────────────────────────────────────────────────
    while let Some(event) = rx.recv().await {
        // D-07: disabled-type branch — mark 'printed' + ack without sending to printer.
        // C4 applies here too: UPDATE must precede ack on every path.
        if !enabled_types.is_empty() && !enabled_types.contains(&event.job_type) {
            // UPDATE before ack (C4) — disabled-type path.
            // WR-02: on UPDATE failure, skip the ack and leave status='pending'.
            // If we acked after a failed UPDATE, the row stays 'pending' with no path to
            // advance it if Noren's queue has already dequeued the job on receipt of the ack.
            match conn.execute(
                "UPDATE printed_jobs SET status='printed', printed_at=datetime('now'), attempt=attempt+1 WHERE job_id=?1",
                rusqlite::params![event.job_id],
            ) {
                Ok(0) => {
                    eprintln!(
                        "[brevly-print] Print worker: UPDATE matched 0 rows for {} (disabled-type) — row absent from DB",
                        event.job_id
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!(
                        "[brevly-print] Print worker: SQLite update failed for {} (disabled-type): {e:#}",
                        event.job_id
                    );
                    continue; // WR-02: skip ack — leave status='pending' for Phase 6 retry
                }
            }
            if let Err(e) = ack_job(&http, &base_url, &agent_token, &event.job_id).await {
                eprintln!(
                    "[brevly-print] Print worker: ack failed (disabled type) for {}: {e:#}",
                    event.job_id
                );
            }
            continue;
        }

        // Fetch ESC/POS bytes from the Noren backend (PRT-01).
        // On failure: leave the row at status='pending' so Phase 6 can retry.
        let bytes = match fetch_job_bytes(&http, &base_url, &agent_token, &event.job_id).await {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "[brevly-print] Print worker: fetch failed for {}: {e:#}",
                    event.job_id
                );
                continue; // leave status='pending' — Phase 6 retries
            }
        };

        // Print (PRT-02/03/04/05/06).
        // C1: call print_raw() only — RAW datatype is hardcoded in the spooler impl.
        // On failure: leave the row at status='pending' so Phase 6 can retry.
        if let Err(e) = printer.print_raw(&bytes) {
            eprintln!(
                "[brevly-print] Print worker: print failed for {}: {e:#}",
                event.job_id
            );
            continue; // leave status='pending' — Phase 6 retries
        }

        // UPDATE before ack — C4 constraint (D-09 / T-05-04).
        // This must textually and temporally precede the ack_job() call below.
        // WR-03: increment attempt counter so Phase 6 retry logic can apply backoff / give up.
        // WR-05: log when rows_affected == 0 (job_id absent — INSERT may have failed silently).
        match conn.execute(
            "UPDATE printed_jobs SET status='printed', printed_at=datetime('now'), attempt=attempt+1 WHERE job_id=?1",
            rusqlite::params![event.job_id],
        ) {
            Ok(0) => eprintln!(
                "[brevly-print] Print worker: UPDATE matched 0 rows for {} — row absent from DB",
                event.job_id
            ),
            Ok(_) => {}
            Err(e) => {
                eprintln!(
                    "[brevly-print] Print worker: SQLite update failed for {}: {e:#}",
                    event.job_id
                );
                // Still proceed to ack — Noren won't resend; status='printed' is best-effort.
            }
        }

        // Ack the job back to Noren (PRT-08).
        // D-09: on ack failure, status is already 'printed'; Phase 6 pending pull handles recovery.
        if let Err(e) = ack_job(&http, &base_url, &agent_token, &event.job_id).await {
            eprintln!(
                "[brevly-print] Print worker: ack failed for {}: {e:#}",
                event.job_id
            );
        }
    }

    eprintln!("[brevly-print] Print worker: channel closed — exiting");
}
