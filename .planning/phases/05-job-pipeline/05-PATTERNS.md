# Phase 5: Job Pipeline - Pattern Map

**Mapped:** 2026-07-16
**Files analyzed:** 6 (4 new/modified source files + 1 new test file + 1 Cargo.toml change)
**Analogs found:** 5 / 6 (Cargo.toml has no analog — trivial addition)

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/print_worker.rs` | service (async task) | event-driven | `src/pusher/client.rs` | exact |
| `src/noren_client.rs` (extend) | service (HTTP client) | request-response | `src/noren_client.rs::pusher_auth()` | exact (same file) |
| `src/lib.rs` (extend) | config | N/A | existing `pub mod` block in `src/lib.rs` | exact |
| `src/main.rs` (modify) | config/entrypoint | N/A | existing Runtime spawn block in `src/main.rs` lines 389–463 | exact |
| `tests/print_worker_test.rs` | test | request-response | `tests/noren_client_test.rs` | exact |
| `Cargo.toml` (extend) | config | N/A | existing `[dependencies]` in `Cargo.toml` | N/A — trivial |

---

## Pattern Assignments

### `src/print_worker.rs` (async task, event-driven)

**Analog:** `src/pusher/client.rs` — `run_pusher_loop()`

**Imports pattern** (`src/pusher/client.rs` lines 29–44):
```rust
use std::path::PathBuf;

use anyhow::Context as _;
use tokio::sync::mpsc;

use crate::{
    config_store,
    noren_client::{fetch_job_bytes, ack_job},
    printer::{printer_from_entry, PrinterId, Printer},
    pusher::protocol::PrintEvent,
};
```
Note: `futures_util`, `tokio_tungstenite`, `interval` are Pusher-specific — omit them. `print_worker.rs` has no WebSocket dependency.

**Function signature pattern** (`src/pusher/client.rs` lines 131–138):
```rust
pub async fn run_print_worker(
    mut rx: mpsc::Receiver<PrintEvent>,
    agent_token: String,
    base_url: String,
    db_path: PathBuf,
    http: reqwest::Client,
)
```
Mirror the `run_pusher_loop` parameter order: credentials/config params after the channel handle, shared client last.

**WAL-mode connection open** (`src/pusher/client.rs` lines 150–161):
```rust
let conn = match rusqlite::Connection::open(&db_path) {
    Ok(c) => c,
    Err(e) => {
        eprintln!("[brevly-print] Print worker: failed to open SQLite connection: {e:#}");
        return;
    }
};
if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
    eprintln!("[brevly-print] Print worker: failed to set WAL mode: {e:#}");
    return;
}
```
Exact copy — log prefix changes from "Pusher task" to "Print worker".

**ConfigStore reads at startup** (`src/config_store.rs` lines 103–110 — the `get()` API):
```rust
// enabled_types: fail-safe default is allow-all (D-03 / Pitfall 5)
let enabled_types: Vec<String> = config_store::get(&conn, "enabled_types")
    .unwrap_or(None)
    .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
    .unwrap_or_default(); // empty = allow all types

// Printer construction
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
let printer = printer_from_entry(&printer_id);
```

**Event loop pattern** (`src/pusher/client.rs` lines 303 ff — `while let` inside inner loop):
```rust
while let Some(event) = rx.recv().await {
    // D-07: enabled_types filter
    if !enabled_types.is_empty() && !enabled_types.contains(&event.job_type) {
        // Disabled type: mark 'printed' + ack, skip print (PRT-09)
        let _ = conn.execute(
            "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
            rusqlite::params![event.job_id],
        );
        if let Err(e) = ack_job(&http, &base_url, &agent_token, &event.job_id).await {
            eprintln!("[brevly-print] Print worker: ack failed (disabled type) for {}: {e:#}", event.job_id);
        }
        continue;
    }

    // Fetch ESC/POS bytes (PRT-01)
    let bytes = match fetch_job_bytes(&http, &base_url, &agent_token, &event.job_id).await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[brevly-print] Print worker: fetch failed for {}: {e:#}", event.job_id);
            continue; // leave status='pending' — Phase 6 retries
        }
    };

    // Print (PRT-02/03/04/05/06)
    if let Err(e) = printer.print_raw(&bytes) {
        eprintln!("[brevly-print] Print worker: print failed for {}: {e:#}", event.job_id);
        continue; // leave status='pending' — Phase 6 retries
    }

    // UPDATE before ack — C4 constraint (D-09)
    if let Err(e) = conn.execute(
        "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
        rusqlite::params![event.job_id],
    ) {
        eprintln!("[brevly-print] Print worker: SQLite update failed for {}: {e:#}", event.job_id);
        // Still attempt ack — Noren won't resend anyway; log and move on
    }

    // Ack (PRT-08)
    if let Err(e) = ack_job(&http, &base_url, &agent_token, &event.job_id).await {
        eprintln!("[brevly-print] Print worker: ack failed for {}: {e:#}", event.job_id);
        // D-09: leave status='printed'; Phase 6 pending pull covers recovery
    }
}
eprintln!("[brevly-print] Print worker: channel closed — exiting");
```

**Logging style** (consistent across `src/pusher/client.rs`):
- Prefix: `[brevly-print]` — exact string, always.
- Context: task name after prefix: `Print worker:`.
- Token: never in log output (T-02-02). Use `.bearer_auth()` only.
- Error display: `{e:#}` (anyhow pretty-print with chain).

---

### `src/noren_client.rs` — `fetch_job_bytes()` (HTTP client, request-response)

**Analog:** `src/noren_client.rs::pusher_auth()` lines 186–221

**Function signature pattern** (mirrors `pusher_auth` lines 186–192):
```rust
pub async fn fetch_job_bytes(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    job_id: &str,
) -> anyhow::Result<Vec<u8>>
```

**Core HTTP pattern** (copy from `pusher_auth` lines 199–220, adapt for GET + base64):
```rust
use base64::Engine as _;

pub async fn fetch_job_bytes(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    job_id: &str,
) -> anyhow::Result<Vec<u8>> {
    #[derive(serde::Deserialize)]
    struct BytesResponse { bytes: String }

    let url = format!("{base_url}/api/agent/jobs/{job_id}/bytes");

    let resp = client
        .get(&url)
        .bearer_auth(agent_token)     // T-02-02: never log agent_token
        .send()
        .await
        .context("fetch_job_bytes: HTTP transport error")?;

    match resp.status().as_u16() {
        200 => {
            let body: BytesResponse = resp
                .json()
                .await
                .context("fetch_job_bytes: response parse error")?;
            base64::engine::general_purpose::STANDARD
                .decode(&body.bytes)
                .context("fetch_job_bytes: base64 decode error")
        }
        status => anyhow::bail!("fetch_job_bytes: unexpected status {status}"),
    }
}
```
Key differences from `pusher_auth`:
- `.get()` not `.post()` / `.form()`
- Response is `Vec<u8>` (base64 decoded) not a `String`
- Local `BytesResponse` struct (same pattern as `PusherAuthResponse` in `pusher_auth`)
- `base64::engine::general_purpose::STANDARD.decode()` — Engine API (Pitfall 6: old free functions removed in 0.22)

---

### `src/noren_client.rs` — `ack_job()` (HTTP client, request-response)

**Analog:** `src/noren_client.rs::pusher_auth()` lines 186–221

**Core HTTP pattern** (mirrors `pusher_auth` status-match block, lines 210–220):
```rust
pub async fn ack_job(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    job_id: &str,
) -> anyhow::Result<()> {
    let url = format!("{base_url}/api/agent/jobs/{job_id}/ack");

    let resp = client
        .post(&url)
        .bearer_auth(agent_token)     // T-02-02: never log agent_token
        .send()
        .await
        .context("ack_job: HTTP transport error")?;

    match resp.status().as_u16() {
        200 | 409 => Ok(()),   // 409 = already acked — idempotent by design (C4 / D-04)
        status => anyhow::bail!("ack_job: unexpected status {status}"),
    }
}
```
Key differences from `pusher_auth`:
- POST with empty body (no `.form()` / `.json()`) — reqwest sends no body by default
- Return type is `()` not `String`
- 409 is `Ok(())` — idempotent (C4 pitfall: a post-crash ack repeat is normal)
- No response body deserialization needed

---

### `src/lib.rs` (extend — add `pub mod print_worker`)

**Analog:** `src/lib.rs` lines 7–16 — existing `pub mod` block

**Pattern** (lines 14–16 of `src/lib.rs`):
```rust
pub mod noren_client;
pub mod printer;
pub mod pusher;
// Add after pusher:
pub mod print_worker;
```
Placement: after `pub mod pusher` (logical sibling — both are background async tasks).

---

### `src/main.rs` (modify — remove `App._print_rx`, spawn print worker)

**Analog:** `src/main.rs` lines 389–449 — existing Pusher spawn block

**Remove from `App` struct** (lines 89–92):
```rust
// DELETE these lines:
/// Receiver for Phase 5 print worker handoff. ...
_print_rx: Option<tokio::sync::mpsc::Receiver<PrintEvent>>,
```

**Remove from `App` initializer** (line 462):
```rust
// DELETE this line from the App { ... } block:
_print_rx: Some(print_rx),
```

**Print worker spawn — add after the existing Pusher spawn block** (after line 448):
```rust
// Phase 5: spawn print worker (D-01)
let worker_db_path = db_path.clone();
let worker_http = http.clone();
let worker_token = agent_token.clone();
let worker_base_url = auth_url.clone();
rt_handle.spawn(async move {
    brevly_print::print_worker::run_print_worker(
        print_rx,
        worker_token,
        worker_base_url,
        worker_db_path,
        worker_http,
    ).await;
});
// print_rx moved into the task — no longer needed here
```
Note: `auth_url` is already in scope at this point (line 420). `agent_token` is already read (lines 425–431). `db_path` is already cloned for the Pusher task (line 440 pattern). Mirror the `pusher_db_path`/`pusher_http` clone pattern.

**Import addition** (line 26 — `brevly_print::pusher` import block):
```rust
// Add print_worker to the brevly_print import block:
use brevly_print::{
    // ... existing imports ...
    print_worker::run_print_worker,
};
```

---

### `tests/print_worker_test.rs` (test, request-response)

**Analog:** `tests/noren_client_test.rs` — full file

**`spawn_stub` helper** (`tests/noren_client_test.rs` lines 21–49):
```rust
// Copy verbatim from tests/noren_client_test.rs — reuse same helper in print_worker_test.rs
async fn spawn_stub(status: u16, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub listener");
    let port = listener.local_addr().unwrap().port();

    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
        socket.write_all(response.as_bytes()).await.expect("write response");
        socket.shutdown().await.ok();
    });

    format!("http://127.0.0.1:{port}")
}
```
Note: The stub in `tests/noren_client_test.rs` serves exactly one request per call. For tests that need two sequential requests (fetch + ack), call `spawn_stub` twice with different responses/ports.

**Test structure pattern** (`tests/noren_client_test.rs` lines 53–73):
```rust
use brevly_print::noren_client::{fetch_job_bytes, ack_job};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use base64::Engine as _;

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
async fn test_ack_job_409_returns_ok() {
    let base_url = spawn_stub(409, r#"{"error":"already acked"}"#).await;
    let client = reqwest::Client::new();
    let result = ack_job(&client, &base_url, "tok-test", "job-001").await;
    assert!(result.is_ok(), "409 must be Ok(()) — idempotent by design (C4)");
}

#[tokio::test]
async fn test_fetch_job_bytes_non_200_returns_err() {
    let base_url = spawn_stub(500, r#"{"error":"server error"}"#).await;
    let client = reqwest::Client::new();
    let result = fetch_job_bytes(&client, &base_url, "tok-test", "job-001").await;
    assert!(result.is_err(), "non-200 must return Err");
}
```

---

## Shared Patterns

### WAL-mode Second Connection
**Source:** `src/pusher/client.rs` lines 150–161
**Apply to:** `src/print_worker.rs` startup
```rust
let conn = match rusqlite::Connection::open(&db_path) {
    Ok(c) => c,
    Err(e) => {
        eprintln!("[brevly-print] Print worker: failed to open SQLite connection: {e:#}");
        return;
    }
};
if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
    eprintln!("[brevly-print] Print worker: failed to set WAL mode: {e:#}");
    return;
}
```

### Bearer Auth — Never Log Token
**Source:** `src/noren_client.rs` lines 202–208 (`pusher_auth`)
**Apply to:** `fetch_job_bytes()`, `ack_job()` in `src/noren_client.rs`
```rust
.bearer_auth(agent_token)  // T-02-02: token passed here, never in eprintln!
```
Rule: `agent_token` must never appear in any format string or error message.

### anyhow::Result + context() Error Chain
**Source:** `src/noren_client.rs` lines 193, 208, 214, 217 (`pusher_auth`)
**Apply to:** `fetch_job_bytes()`, `ack_job()`
```rust
.context("fetch_job_bytes: HTTP transport error")?;
// and
.context("fetch_job_bytes: response parse error")?;
// and
.context("fetch_job_bytes: base64 decode error")
```
Pattern: every `?`-propagated error gets a `.context("fn_name: description")` prefix so the anyhow error chain is navigable.

### Parameterised SQL — No String Interpolation
**Source:** `src/pusher/client.rs` lines 64–69 (`insert_print_job`) and `src/config_store.rs` lines 87–94
**Apply to:** All `conn.execute()` calls in `src/print_worker.rs`
```rust
conn.execute(
    "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
    rusqlite::params![event.job_id],
)?;
```
Never interpolate `event.job_id` into the SQL string (T-04-07).

### eprintln! Logging Style
**Source:** `src/pusher/client.rs` — every `eprintln!` call
**Apply to:** `src/print_worker.rs`
```rust
eprintln!("[brevly-print] Print worker: <message>: {e:#}");
```
- Always `[brevly-print]` prefix.
- Task name: `Print worker:` (matches `Pusher task:` / `Pusher:` style).
- `{e:#}` for error display (anyhow chain).
- No sensitive data (token, raw bytes) in any log output.

### Mock TCP Stub for HTTP Tests
**Source:** `tests/noren_client_test.rs` lines 21–49
**Apply to:** `tests/print_worker_test.rs`

The `spawn_stub(status, body)` function is the established pattern for all HTTP contract tests in this project. Copy it verbatim into `tests/print_worker_test.rs` (or extract into `tests/common/mod.rs` if a third test file needs it).

---

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `Cargo.toml` (base64 dep) | config | N/A | No analog needed — `base64 = "0.22"` is a one-line addition to `[dependencies]`. Already present in `Cargo.lock` as transitive dep (version 0.22.1). |

---

## Critical Constraints for Planner

These constraints appear in STATE.md and must be encoded as explicit ordering rules in the plan:

| Constraint | Source | Enforcement |
|------------|--------|-------------|
| C4: UPDATE before ack | STATE.md + D-09 | `conn.execute(UPDATE ...)` must precede `ack_job()` call in every code path including the disabled-type path |
| C1: RAW datatype in WritePrinter | STATE.md | Do not alter `printer.print_raw()` call or add intermediate layers; `"RAW"` is hardcoded in `WindowsSpoolerPrinter::print_raw()` |
| C2: no tray access from async task | STATE.md | `run_print_worker` must not hold `EventLoopProxy` or `TrayIcon`; Phase 5 logs only |
| T-02-02: no token in logs | Security | `agent_token` parameter never formatted into any `eprintln!` string |
| Pitfall 5: enabled_types fail-safe | RESEARCH.md | Missing/empty `enabled_types` → `unwrap_or_default()` → allow all types |

---

## Metadata

**Analog search scope:** `src/`, `tests/`
**Files scanned:** 6 source files + 1 test file read in full
**Pattern extraction date:** 2026-07-16
