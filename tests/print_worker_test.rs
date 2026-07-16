//! Contract tests for `noren_client::fetch_job_bytes` and `noren_client::ack_job`.
//!
//! Wave-0 scaffold — tests compile once `fetch_job_bytes` and `ack_job` are
//! implemented in Task 2.  Uses a mock TCP listener (no live Noren endpoint).
//!
//! Covers the four behaviors from 05-01-PLAN.md:
//!   - 200 + base64 body → Ok(decoded Vec<u8>)
//!   - 500 → Err (non-200 status)
//!   - 409 → Ok(()) — idempotent ack (C4)
//!   - 500 on ack → Err

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
