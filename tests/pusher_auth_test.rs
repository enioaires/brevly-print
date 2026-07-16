//! Integration tests for `pusher_auth()` in `noren_client`.
//!
//! HTTP contract tests: 200 → Ok(auth_string), 403 → Err, transport → Err.
//!
//! **Linux-testable** — uses the same mock TCP stub pattern as `noren_client_test.rs`.
//! No live Noren endpoint or Pusher connection required.
//!
//! Covers: EVT-01 (auth contract), EVT-02 (fresh-POST contract).

use brevly_print::noren_client::pusher_auth;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

/// Spawn a local HTTP stub that returns `body` with the given status code.
/// Returns the base URL (e.g. `"http://127.0.0.1:PORT"`).
///
/// The stub listens for one request, sends the canned response, then closes.
/// Copied from `tests/noren_client_test.rs` — identical helper.
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
        socket.shutdown().await.ok();
    });

    format!("http://127.0.0.1:{port}")
}

// ── 200 OK → Ok(auth_string) ────────────────────────────────────────────────

#[tokio::test]
async fn test_pusher_auth_200_returns_auth_string() {
    let body = r#"{"auth":"key123:hmac-abc"}"#;
    let base_url = spawn_stub(200, body).await;

    let client = reqwest::Client::new();
    let result = pusher_auth(
        &client,
        &base_url,
        "tok-xyz",
        "private-tenant-t1-print",
        "123.456",
    )
    .await;

    assert_eq!(
        result.unwrap(),
        "key123:hmac-abc",
        "200 should return the auth string from the response body"
    );
}

// ── 403 → Err ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_pusher_auth_403_returns_err() {
    let base_url = spawn_stub(403, r#"{"error":"forbidden"}"#).await;

    let client = reqwest::Client::new();
    let result = pusher_auth(
        &client,
        &base_url,
        "tok-invalid",
        "private-tenant-t1-print",
        "123.456",
    )
    .await;

    assert!(
        result.is_err(),
        "403 should return Err; got: {result:?}"
    );
    // Verify the error message mentions 403 (not the token — T-04-01 security check)
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("403"),
        "error message should mention 403, got: {err_msg}"
    );
    assert!(
        !err_msg.contains("tok-invalid"),
        "error message must not contain the agent token (T-04-01)"
    );
}

// ── Connection refused → Err (transport) ─────────────────────────────────────

#[tokio::test]
async fn test_pusher_auth_connection_refused_returns_err() {
    // Bind a port, record it, drop the listener immediately — port now refuses connections.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind for port discovery");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let base_url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let result = pusher_auth(
        &client,
        &base_url,
        "tok-xyz",
        "private-tenant-t1-print",
        "123.456",
    )
    .await;

    assert!(
        result.is_err(),
        "connection refused should return Err; got: {result:?}"
    );
}
