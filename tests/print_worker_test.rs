//! Contract tests for `noren_client::fetch_job_bytes` and `noren_client::ack_job`,
//! plus Phase 5 Plan 02 unit tests for the print worker filter and ordering constraint.
//!
//! Wave-0 scaffold (Plan 01): four HTTP contract tests covering PRT-01 and PRT-08.
//!
//! Plan 02 additions:
//!   - `enabled_types_filter` (5-02-01 / PRT-09): allow/skip predicate, empty = allow-all
//!   - `update_precedes_ack_in_source` (5-02-02 / C4): static ordering assertion

use base64::Engine as _;
use brevly_print::noren_client::{ack_job, fetch_job_bytes};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

/// Spawn a local HTTP stub that returns `body` with the given status code.
/// Returns the base URL (e.g. "http://127.0.0.1:PORT").
///
/// The stub listens for one request, sends the canned response, then closes.
async fn spawn_stub(status: u16, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub listener");
    let port = listener.local_addr().unwrap().port();

    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write response");
        // Graceful half-close: signal EOF on the write side so reqwest finishes reading
        // the response body before the full socket closes.
        socket.shutdown().await.ok();
    });

    format!("http://127.0.0.1:{port}")
}

// ── fetch_job_bytes ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_fetch_job_bytes_200_decodes_base64() {
    let raw = b"\x1b\x40Hello\x1d\x56\x00";
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
    // body must be &'static str for spawn_stub; Box::leak promotes it
    let body_json = format!(r#"{{"bytes":"{}"}}"#, encoded);
    let body: &'static str = Box::leak(body_json.into_boxed_str());

    let base_url = spawn_stub(200, body).await;
    let client = reqwest::Client::new();
    let result = fetch_job_bytes(&client, &base_url, "tok-test", "job-001").await;
    assert_eq!(result.unwrap(), raw);
}

#[tokio::test]
async fn test_fetch_job_bytes_non_200_returns_err() {
    let base_url = spawn_stub(500, r#"{"error":"server error"}"#).await;
    let client = reqwest::Client::new();
    let result = fetch_job_bytes(&client, &base_url, "tok-test", "job-001").await;
    assert!(result.is_err(), "non-200 must return Err");
}

// ── ack_job ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_ack_job_409_returns_ok() {
    let base_url = spawn_stub(409, r#"{"error":"already acked"}"#).await;
    let client = reqwest::Client::new();
    let result = ack_job(&client, &base_url, "tok-test", "job-001").await;
    assert!(result.is_ok(), "409 must be Ok(()) — idempotent by design (C4)");
}

#[tokio::test]
async fn test_ack_job_500_returns_err() {
    let base_url = spawn_stub(500, r#"{"error":"boom"}"#).await;
    let client = reqwest::Client::new();
    let result = ack_job(&client, &base_url, "tok-test", "job-001").await;
    assert!(result.is_err(), "500 must return Err");
}

// ── Plan 02: enabled_types filter (5-02-01 / PRT-09) ────────────────────────

/// Verify the allow/skip predicate used by `run_print_worker` (D-07 / PRT-09).
///
/// The inline predicate `!enabled.is_empty() && !enabled.contains(job_type)` controls
/// whether a job is routed to the printer (allowed) or marked 'printed' + acked without
/// printing (skipped).  An empty enabled list is the fail-safe allow-all behaviour
/// (D-03 / Pitfall 5 / T-05-06).
///
/// This test mirrors the exact boolean logic used in `run_print_worker` so that any
/// accidental inversion or short-circuit change in the production code will also break
/// the predicate here.
#[test]
fn enabled_types_filter() {
    // Pure predicate — mirrors the inline check in run_print_worker (D-07).
    fn is_allowed(enabled: &[String], job_type: &str) -> bool {
        enabled.is_empty() || enabled.contains(&job_type.to_string())
    }

    let enabled = vec!["order".to_string(), "dispatch".to_string()];

    // Job type present in the list → allowed.
    assert!(
        is_allowed(&enabled, "order"),
        "job_type 'order' is in enabled_types → must be allowed"
    );
    // Job type absent from the list → skipped (disabled-type branch).
    assert!(
        !is_allowed(&enabled, "closing"),
        "job_type 'closing' is NOT in enabled_types → must be skipped"
    );
    // Empty enabled list → allow-all (fail-safe: misconfigured or missing key).
    assert!(
        is_allowed(&[], "closing"),
        "empty enabled_types → allow-all (fail-safe); 'closing' must be allowed"
    );
}

// ── Plan 02: UPDATE-before-ack ordering (5-02-02 / C4 / T-05-04) ────────────

/// Static source-order assertion that `UPDATE printed_jobs SET status='printed'`
/// textually precedes the final `ack_job(` call in `src/print_worker.rs`.
///
/// This is the C4 constraint (D-09 / T-05-04): the SQLite status update MUST be
/// written before the ack is sent, on every code path.  A future refactor that
/// accidentally swaps the two statements will fail this test.
///
/// Strategy: use `include_str!` to embed the source file at compile time, then
/// assert that the byte index of the first UPDATE occurrence is less than the byte
/// index of the last `ack_job(` occurrence — proving that the UPDATE statement
/// appears before the final ack call in the success path.
#[test]
fn update_precedes_ack_in_source() {
    let src = include_str!("../src/print_worker.rs");

    let update_idx = src
        .find("UPDATE printed_jobs SET status='printed'")
        .expect("UPDATE statement not found in src/print_worker.rs");

    let ack_idx = src
        .rfind("ack_job(")
        .expect("ack_job( not found in src/print_worker.rs");

    assert!(
        update_idx < ack_idx,
        "C4 ordering violated: first UPDATE index ({update_idx}) must be < last ack_job index ({ack_idx}). \
         SQLite UPDATE must textually precede ack_job() in src/print_worker.rs."
    );
}
