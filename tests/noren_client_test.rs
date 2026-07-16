//! Integration tests for `noren_client`: status-code → ActivateError mapping.
//!
//! **Linux-testable** — uses a mock TCP listener that returns canned HTTP responses.
//! No live Noren endpoint is required (Open Question 5).
//!
//! Covers the five behaviors specified in 02-01-PLAN.md Task 1:
//!   - 200 → Ok(ActivateResponse) with all fields populated
//!   - 404 → Err(ActivateError::InvalidSerial)
//!   - 403 → Err(ActivateError::InvalidSerial)
//!   - 409 → Err(ActivateError::AlreadyActiveOther)
//!   - connection refused → Err(ActivateError::Transport(_))

use brevly_print::noren_client::{activate, ActivateError};
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
        // Drain the request headers (read and discard)
        let mut buf = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write response");
    });

    format!("http://127.0.0.1:{port}")
}

// ── 200 OK → Ok(ActivateResponse) ──────────────────────────────────────────

#[tokio::test]
async fn test_activate_200_returns_response() {
    let body = r#"{
        "agentToken": "tok-abc123",
        "tenantId": "tenant-xyz",
        "pusherKey": "key-456",
        "pusherCluster": "mt1",
        "enabledTypes": ["pedido", "despacho"]
    }"#;
    let base_url = spawn_stub(200, body).await;

    let client = reqwest::Client::new();
    let result = activate(&client, &base_url, "SERIAL-001", None, false).await;

    let resp = result.expect("200 should return Ok");
    assert_eq!(resp.agent_token, "tok-abc123");
    assert_eq!(resp.tenant_id, "tenant-xyz");
    assert_eq!(resp.pusher_key, "key-456");
    assert_eq!(resp.pusher_cluster, "mt1");
    assert_eq!(resp.enabled_types, vec!["pedido", "despacho"]);
}

// ── 404 → Err(InvalidSerial) ──────────────────────────────────────────────

#[tokio::test]
async fn test_activate_404_returns_invalid_serial() {
    let base_url = spawn_stub(404, r#"{"error":"not found"}"#).await;

    let client = reqwest::Client::new();
    let result = activate(&client, &base_url, "SERIAL-BAD", None, false).await;

    assert!(
        matches!(result, Err(ActivateError::InvalidSerial)),
        "expected InvalidSerial, got: {result:?}"
    );
}

// ── 403 → Err(InvalidSerial) ──────────────────────────────────────────────

#[tokio::test]
async fn test_activate_403_returns_invalid_serial() {
    let base_url = spawn_stub(403, r#"{"error":"forbidden"}"#).await;

    let client = reqwest::Client::new();
    let result = activate(&client, &base_url, "SERIAL-BAD", None, false).await;

    assert!(
        matches!(result, Err(ActivateError::InvalidSerial)),
        "expected InvalidSerial, got: {result:?}"
    );
}

// ── 409 → Err(AlreadyActiveOther) ─────────────────────────────────────────

#[tokio::test]
async fn test_activate_409_returns_already_active_other() {
    let base_url = spawn_stub(409, r#"{"error":"conflict"}"#).await;

    let client = reqwest::Client::new();
    let result = activate(&client, &base_url, "SERIAL-TAKEN", None, false).await;

    assert!(
        matches!(result, Err(ActivateError::AlreadyActiveOther)),
        "expected AlreadyActiveOther, got: {result:?}"
    );
}

// ── Connection refused → Err(Transport(_)) ────────────────────────────────

#[tokio::test]
async fn test_activate_connection_refused_returns_transport() {
    // Bind to a random port, record it, then drop the listener immediately.
    // reqwest will fail to connect → Transport error.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind for port discovery");
    let port = listener.local_addr().unwrap().port();
    drop(listener); // Close immediately — port now refuses connections

    let base_url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let result = activate(&client, &base_url, "SERIAL-001", None, false).await;

    assert!(
        matches!(result, Err(ActivateError::Transport(_))),
        "expected Transport, got: {result:?}"
    );
}
