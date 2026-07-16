//! RED stubs for Phase 6 RES-03: pending job pull on Pusher reconnect.
//!
//! Wave 0 scaffold — tests that call the not-yet-existing
//! `brevly_print::noren_client::fetch_pending_jobs` are gated behind
//! `#[ignore = "Wave 3: fetch_pending_jobs not yet implemented"]` so this file
//! compiles today.  Wave 3 removes the ignore attributes and implements the
//! production function.
//!
//! Noren API contract (VERIFIED from ~/repos/brevly/noren/src/routes/api/agent/jobs/pending/+server.ts):
//! Response body is a WRAPPER OBJECT, not a bare array:
//!   `{ "jobs": [ { "jobId": "abc", "type": "pedido" }, ... ] }`
//! Each element uses camelCase keys `jobId` (not `job_id`) and `type` (not `job_type`).
//! Auth: Bearer agentToken.  403 on invalid/missing token.
//!
//! Portable: runs on Linux and Windows (no Windows-API dependency in this file).

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

// ── HTTP stub helper (copied from tests/noren_client_test.rs) ────────────────

/// Spawn a local HTTP stub that returns `body` with the given status code.
/// Returns the base URL (e.g. `"http://127.0.0.1:PORT"`).
///
/// The stub listens for one request, sends the canned response, then closes.
/// For dynamic JSON bodies, promote with `Box::leak(format!(...).into_boxed_str())`
/// to obtain a `&'static str`.
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
        // Drain request headers (fixed-size read is enough for test payloads).
        // Shut down the write half before dropping so reqwest sees a clean EOF
        // rather than a connection-reset when the socket closes while it may
        // still be writing.
        let mut buf = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write response");
        // Graceful half-close: signal EOF on the write side.
        socket.shutdown().await.ok();
    });

    format!("http://127.0.0.1:{port}")
}

// ── RES-03: fetch_pending_jobs 200 (Wave 3) ──────────────────────────────────

/// Assert `fetch_pending_jobs` parses the Noren wrapper-object response correctly.
///
/// D-09 contract: response is `{"jobs":[...]}` (NOT a bare array).
/// Each element has camelCase keys `jobId` and `type`.
///
/// After fetching, the Pusher task must call `insert_print_job` for each job
/// and route new jobs through the existing mpsc channel (D-08).
#[tokio::test]
#[ignore = "Wave 3: fetch_pending_jobs not yet implemented"]
async fn fetch_pending_jobs_200_parses_wrapper() {
    let body = r#"{"jobs":[{"jobId":"job-1","type":"pedido"},{"jobId":"job-2","type":"despacho"}]}"#;
    let base_url = spawn_stub(200, body).await;
    let client = reqwest::Client::new();

    // When fetch_pending_jobs is implemented in src/noren_client.rs, remove
    // #[ignore] and replace the todo!() with the real call:
    //
    //   use brevly_print::noren_client::fetch_pending_jobs;
    //   let jobs = fetch_pending_jobs(&client, &base_url, "tok-test")
    //       .await
    //       .expect("200 must return Ok");
    //   assert_eq!(jobs.len(), 2, "wrapper must yield 2 jobs");
    //   assert_eq!(jobs[0].job_id, "job-1");
    //   assert_eq!(jobs[0].job_type, "pedido");
    //   assert_eq!(jobs[1].job_id, "job-2");
    //   assert_eq!(jobs[1].job_type, "despacho");
    //
    // Verify: response is a WRAPPER OBJECT `{"jobs":[...]}` with `jobId`/`type`
    // keys (D-09) — NOT a bare array and NOT snake_case.
    let _ = (client, base_url);
    todo!(
        "Wave 3: implement fetch_pending_jobs in src/noren_client.rs (D-08/D-09), \
         then remove #[ignore] and activate the assertions above"
    )
}

// ── RES-03: fetch_pending_jobs empty array (Wave 3) ──────────────────────────

/// Assert `fetch_pending_jobs` returns an empty Vec (not Err) when the server
/// returns `{"jobs":[]}`.  An empty pending list is a valid state (no missed jobs).
#[tokio::test]
#[ignore = "Wave 3: fetch_pending_jobs not yet implemented"]
async fn fetch_pending_jobs_empty_array() {
    let body = r#"{"jobs":[]}"#;
    let base_url = spawn_stub(200, body).await;
    let client = reqwest::Client::new();

    // When implemented:
    //   use brevly_print::noren_client::fetch_pending_jobs;
    //   let jobs = fetch_pending_jobs(&client, &base_url, "tok-test")
    //       .await
    //       .expect("empty jobs must return Ok, not Err");
    //   assert!(jobs.is_empty(), "empty jobs array must yield an empty Vec");
    let _ = (client, base_url);
    todo!(
        "Wave 3: remove #[ignore] after implementing fetch_pending_jobs"
    )
}

// ── RES-03: fetch_pending_jobs non-200 (Wave 3) ──────────────────────────────

/// Assert `fetch_pending_jobs` returns Err on a 5xx response.
///
/// D-08 error handling: on failure, log and continue — do not reconnect the
/// WebSocket.  The pending pull is best-effort; the next reconnect will retry.
#[tokio::test]
#[ignore = "Wave 3: fetch_pending_jobs not yet implemented"]
async fn fetch_pending_jobs_non_200_returns_err() {
    let body = r#"{"error":"internal server error"}"#;
    let base_url = spawn_stub(500, body).await;
    let client = reqwest::Client::new();

    // When implemented:
    //   use brevly_print::noren_client::fetch_pending_jobs;
    //   let result = fetch_pending_jobs(&client, &base_url, "tok-test").await;
    //   assert!(result.is_err(), "non-200 must return Err");
    let _ = (client, base_url);
    todo!(
        "Wave 3: remove #[ignore] after implementing fetch_pending_jobs"
    )
}

// ── CR-02: validate_job_id traversal invariant (active, non-ignored) ─────────

/// Document the CR-02 security invariant: path traversal characters in a
/// `job_id` from `fetch_pending_jobs` must be rejected BEFORE the job_id is
/// used to construct a URL path segment.
///
/// `validate_job_id` in `src/noren_client.rs` is `pub(crate)` and cannot be
/// called from an integration test.  Instead, this test asserts the invariant at
/// the string level: a crafted id like `"../admin"` contains forbidden characters
/// that `validate_job_id` is specified to reject.
///
/// Wave 3 wiring MUST call `validate_job_id` on every `job.job_id` returned by
/// `fetch_pending_jobs` before passing it to `insert_print_job` or
/// `fetch_job_bytes` (CR-02).
#[test]
fn validate_job_id_rejects_traversal_documented() {
    // Characters that validate_job_id rejects (per src/noren_client.rs ~line 172).
    let forbidden: &[char] = &['/', '.', '\\', '?', '#', '%', '\0'];

    // Adversarial job_ids that should be rejected by validate_job_id:
    let bad_ids = [
        "../admin",
        "/etc/passwd",
        "foo%2Fbar",
        "job?query=x",
        "job#anchor",
        "job\\win",
        "job\0null",
        "../../secret",
    ];

    for id in &bad_ids {
        let contains_forbidden = id.chars().any(|c| forbidden.contains(&c));
        assert!(
            contains_forbidden,
            "adversarial id {:?} must contain at least one forbidden character — \
             validate_job_id in src/noren_client.rs MUST reject it (CR-02)",
            id
        );
    }

    // A valid job_id (alphanumeric + hyphens) must NOT trigger the forbidden check.
    let good_id = "job-abc123-pedido";
    let good_is_clean = !good_id.chars().any(|c| forbidden.contains(&c));
    assert!(
        good_is_clean,
        "valid job_id {:?} must NOT contain forbidden characters",
        good_id
    );

    // Wave 3 pusher wiring MUST call validate_job_id on every job.job_id from
    // fetch_pending_jobs before any URL construction or DB insert (CR-02 / T-06-01).
}
