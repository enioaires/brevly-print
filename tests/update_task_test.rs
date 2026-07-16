//! Integration tests for Phase 7 update logic.
//!
//! Portable — runs on Linux via `cargo test --test update_task_test` (D-07).
//! Pure-function tests (check_for_update, verify_sha256) need no mock.
//! HTTP tests for check_version and try_check_and_stage use the same
//! spawn_stub pattern as noren_client_test.rs.
//!
//! Test inventory:
//! 1. check_version_200_parses_camel_case  — 200 + camelCase → VersionResponse fields correct
//! 2. check_version_500_returns_err        — non-200 → Err (no panic)
//! 3. sc2_mismatch_aborts_without_staging  — SHA256 mismatch → Ok(false), stage not called
//! 4. try_check_and_stage_bad_json_returns_err — malformed JSON → Err (no panic)

use brevly_print::noren_client::check_version;
use brevly_print::update::check::{check_for_update, UpdateDecision};
use brevly_print::update::verify::verify_sha256;
use brevly_print::update::try_check_and_stage;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

// ── Shared helper ─────────────────────────────────────────────────────────────

/// Spawn a local HTTP stub that returns `body` with the given status code.
/// Returns the base URL (e.g. "http://127.0.0.1:PORT").
///
/// The stub listens for one request, sends the canned response, then closes.
/// Copied verbatim from tests/noren_client_test.rs lines 21–49.
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
        // Drain request headers; shut down write half for clean EOF.
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

/// Spawn a stub that serves arbitrary bytes (for artifact download endpoint).
/// Returns the full URL including a `/artifact.bin` path (mimicking a download URL).
async fn spawn_bytes_stub(bytes: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind bytes stub");
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        );
        socket
            .write_all(header.as_bytes())
            .await
            .expect("write header");
        socket.write_all(&bytes).await.expect("write body");
        socket.shutdown().await.ok();
    });

    format!("http://127.0.0.1:{port}/artifact.bin")
}

// ── Pure-function smoke tests (no HTTP mock needed) ───────────────────────────

#[test]
fn pure_check_for_update_newer_returns_available() {
    assert!(matches!(
        check_for_update("0.1.0", "0.2.0"),
        UpdateDecision::UpdateAvailable
    ));
}

#[test]
fn pure_verify_sha256_match_returns_ok() {
    use sha2::{Digest, Sha256};
    let data = b"test-artifact-bytes";
    let hex: String = Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(verify_sha256(data, &hex).is_ok());
}

// ── Test 1: check_version happy path ─────────────────────────────────────────

#[tokio::test]
async fn check_version_200_parses_camel_case() {
    let body = r#"{"latest":"0.2.0","downloadUrl":"http://example.com/pkg.nupkg","sha256":"deadbeef"}"#;
    let base_url = spawn_stub(200, body).await;

    let client = reqwest::Client::new();
    let result = check_version(&client, &base_url, "test-token").await;

    let ver = result.expect("200 should return Ok(VersionResponse)");
    assert_eq!(ver.version, "0.2.0");
    assert_eq!(ver.download_url, "http://example.com/pkg.nupkg");
    assert_eq!(ver.sha256, "deadbeef");
}

// ── Test 2: check_version non-200 → Err ──────────────────────────────────────

#[tokio::test]
async fn check_version_500_returns_err() {
    let base_url = spawn_stub(500, r#"{"error":"internal"}"#).await;

    let client = reqwest::Client::new();
    let result = check_version(&client, &base_url, "test-token").await;

    assert!(result.is_err(), "500 should return Err, got: {result:?}");
}

// ── Test 3: SC-2 mismatch abort ───────────────────────────────────────────────
//
// Drive try_check_and_stage against:
// - Version stub: returns a version NEWER than CARGO_PKG_VERSION (0.99.9) so the
//   update path is taken, with a downloadUrl pointing to the artifact stub.
// - Artifact stub: serves real bytes but the sha256 in the version response is WRONG.
//
// Expected: try_check_and_stage returns Ok(false) — mismatch aborted, no stage.
// This proves the Windows stage_update path is never reached on Linux and no
// UpdateStaged signal would be produced (SC-2).

#[tokio::test]
async fn sc2_mismatch_aborts_without_staging() {
    // Serve the artifact bytes (sha256 will be intentionally wrong).
    let artifact_bytes = b"fake-artifact-content".to_vec();
    let artifact_url = spawn_bytes_stub(artifact_bytes.clone()).await;

    // Build the version stub response with a WRONG sha256.
    let wrong_sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
    let version_body = Box::leak(
        format!(
            r#"{{"latest":"0.99.9","downloadUrl":"{artifact_url}","sha256":"{wrong_sha256}"}}"#
        )
        .into_boxed_str(),
    );

    let base_url = spawn_stub(200, version_body).await;

    let client = reqwest::Client::new();
    let result = try_check_and_stage(&client, &base_url, "test-token").await;

    // Must return Ok(false) — mismatch aborted, NOT Ok(true) (no staging).
    assert!(
        matches!(result, Ok(false)),
        "SC-2: mismatch must abort with Ok(false), got: {result:?}"
    );
    // Extra guard: NOT Ok(true) — the stage path must NOT have been reached.
    assert!(
        !matches!(result, Ok(true)),
        "SC-2 VIOLATION: try_check_and_stage returned Ok(true) on mismatch"
    );
}

// ── Test 4: DoS / no-panic on malformed JSON ──────────────────────────────────
//
// try_check_and_stage against a stub that returns malformed JSON (simulates
// a bad /api/agent/version response). Must return Err (not panic).

#[tokio::test]
async fn try_check_and_stage_bad_json_returns_err() {
    let base_url = spawn_stub(200, "not-valid-json{{{").await;

    let client = reqwest::Client::new();
    let result = try_check_and_stage(&client, &base_url, "test-token").await;

    assert!(
        result.is_err(),
        "malformed JSON should return Err, got: {result:?}"
    );
}
