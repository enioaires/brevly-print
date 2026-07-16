# Phase 4: Pusher Event Stream - Pattern Map

**Mapped:** 2026-07-16
**Files analyzed:** 7 new/modified files
**Analogs found:** 7 / 7

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/pusher/mod.rs` | module index | — | `src/printer/mod.rs` | role-match (module pub-use index) |
| `src/pusher/client.rs` | service | event-driven (WebSocket reconnect loop) | `src/noren_client.rs` | role-match (async HTTP service → async WS service) |
| `src/pusher/protocol.rs` | utility (parser/types) | transform | `src/health_state.rs` | role-match (typed domain structs + pure logic) |
| `src/pusher/backoff.rs` | utility (pure fn) | transform | `src/health_state.rs` | role-match (pure logic module with inline tests) |
| `src/noren_client.rs` | service | request-response | `src/noren_client.rs` (self) | exact — adding `pusher_auth()` function |
| `src/main.rs` | entrypoint / wiring | event-driven | `src/main.rs` (self) | exact — extending Runtime mode spawn site |
| `tests/pusher_auth_test.rs` | test | request-response | `tests/noren_client_test.rs` | exact (mock TCP stub pattern) |

---

## Pattern Assignments

### `src/pusher/mod.rs` (module index)

**Analog:** `src/printer/mod.rs` (lines 1–19)

**Module declaration pattern** (`src/printer/mod.rs` lines 1–19):
```rust
//! Printer: trait + cfg-gated platform implementations.
//! ...

pub mod error;

#[cfg(windows)]
mod spooler;

#[cfg(windows)]
mod serial;

#[cfg(not(windows))]
mod stub;

pub use error::PrinterError;
```

**Apply to `src/pusher/mod.rs`:** The Pusher module itself is cross-platform (no `#[cfg]` gate needed — `tokio-tungstenite` compiles on Linux). Use a flat pub-use re-export pattern:

```rust
//! Pusher Channels WebSocket client — reconnect loop, channel auth, event dispatch.
//!
//! Cross-platform: no `#[cfg(windows)]` guard needed.
//! The spawn site in `main.rs` Runtime mode is already in a Windows-only context.

pub mod client;
pub mod protocol;
pub mod backoff;

pub use client::run_pusher_loop;
pub use protocol::{PrintEvent, PusherConfig};
```

---

### `src/pusher/protocol.rs` (utility, transform)

**Analog:** `src/health_state.rs` — typed domain structs + inline unit tests

**Struct + inline test pattern** (`src/health_state.rs` lines 1–84):
```rust
//! Health state machine for the tray agent.
//!
//! Portable — no `#[cfg(windows)]` on the enum or string mappings.

/// Tri-color connection state reflected in the tray icon (RUN-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    Connected,
    Reconnecting,
    Problem,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_states_have_distinct_tooltips() {
        // ...
    }
}
```

**Apply to `src/pusher/protocol.rs`:** Portable module (no cfg gate). Inline `#[cfg(test)] mod tests` with unit tests for parse/decode functions. Key types to define:

```rust
#[derive(serde::Deserialize, Debug)]
pub struct PusherEnvelope {
    pub event: String,
    pub data: serde_json::Value,  // String for system events; double-encoded for app events
    #[serde(default)]
    pub channel: Option<String>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct PrintEvent {
    #[serde(rename = "jobId")]
    pub job_id: String,
    #[serde(rename = "type")]
    pub job_type: String,
}

/// Config bundle passed to the Pusher reconnect loop.
#[derive(Clone)]
pub struct PusherConfig {
    pub key: String,
    pub cluster: String,
    pub tenant_id: String,
    pub auth_url: String,  // noren_base_url() value
}
```

**Double-decode pattern** (from RESEARCH.md Pattern 3):
```rust
// After outer envelope parsed: env.data is a JSON-encoded STRING for app events.
fn parse_print_job(env: &PusherEnvelope) -> anyhow::Result<PrintEvent> {
    let data_str = match &env.data {
        serde_json::Value::String(s) => s.as_str(),
        other => return Err(anyhow::anyhow!("Expected JSON string in data, got: {other}")),
    };
    serde_json::from_str::<PrintEvent>(data_str)
        .context("Failed to decode print job payload (double-decode)")
}
```

**`connection_established` double-decode** (same pattern — also JSON-in-JSON):
```rust
#[derive(serde::Deserialize)]
struct ConnectionEstablishedData {
    socket_id: String,
}

pub fn extract_socket_id(env: &PusherEnvelope) -> anyhow::Result<String> {
    let data_str = env.data.as_str()
        .ok_or_else(|| anyhow::anyhow!("connection_established data is not a string"))?;
    let data: ConnectionEstablishedData = serde_json::from_str(data_str)?;
    Ok(data.socket_id)
}
```

---

### `src/pusher/backoff.rs` (utility, transform)

**Analog:** `src/health_state.rs` — pure logic module with `#[cfg(test)] mod tests`

**Pattern:** Self-contained pure function + inline tests.

```rust
//! Exponential backoff with jitter for the Pusher reconnect loop.
//!
//! D-06: initial 1 s, doubles each attempt, 60 s cap, ±25% jitter.
//! Jitter does not extend the result beyond the 60 s cap.

use std::time::Duration;

/// Compute the delay before the next reconnect attempt.
///
/// - Base: 1s * 2^attempt, capped at 60s before jitter
/// - Jitter: ±25% random multiplier (requires `rand` dep or std::time modulo approach)
/// - Final result is also capped at 60s
pub fn backoff_delay(attempt: u32) -> Duration {
    let base_ms: u64 = 1000u64.saturating_mul(1u64 << attempt.min(6)); // cap base at 64s
    let capped_ms = base_ms.min(60_000);
    // Jitter implementation: planner chooses rand crate or std::time modulo
    Duration::from_millis(capped_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_never_exceeds_60s() {
        for attempt in 0..=20 {
            assert!(backoff_delay(attempt).as_millis() <= 60_000,
                "attempt {attempt} exceeded 60s cap");
        }
    }
}
```

---

### `src/pusher/client.rs` (service, event-driven)

**Analog:** `src/noren_client.rs` — async function taking `&reqwest::Client`, `anyhow::Result` error handling, `context()` chaining.

**Imports pattern** (`src/noren_client.rs` lines 1–13 + 12–17):
```rust
//! module-level doc comment

use serde::{Deserialize, Serialize};
use thiserror::Error;
// + for client.rs:
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{StreamExt, SinkExt};
use anyhow::Context as _;
```

**Async service function pattern** (`src/noren_client.rs` lines 110–142):
```rust
pub async fn activate(
    client: &reqwest::Client,
    base_url: &str,
    serial: &str,
    // ...
) -> Result<ActivateResponse, ActivateError> {
    let url = format!("{base_url}/api/agent/activate");
    let resp = client
        .post(&url)
        .json(&ActivateRequest { serial, machine_id, force_rebind })
        .send()
        .await
        .map_err(ActivateError::Transport)?;
    match resp.status().as_u16() {
        200 => resp.json::<ActivateResponse>().await.map_err(ActivateError::Transport),
        403 | 404 => Err(ActivateError::InvalidSerial),
        // ...
    }
}
```

**Apply to `src/pusher/client.rs`:** Top-level entry point is `pub async fn run_pusher_loop(...)` (the reconnect loop), which never returns (runs for process lifetime). Internal async helpers follow the `noren_client.rs` pattern of named async fns taking shared refs.

**Background→tray proxy pattern** (`src/main.rs` lines 363–373):
```rust
// Established pattern for sending events to the event-loop thread:
#[cfg(windows)]
{
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::TrayIconEvent(event));
    }));
}
```
In `client.rs`, the Pusher task calls:
```rust
let _ = proxy.send_event(UserEvent::HealthChanged(HealthState::Reconnecting));
// or:
let _ = proxy.send_event(UserEvent::HealthChanged(HealthState::Connected));
```
The `let _ =` pattern (ignore send errors) matches exactly what is done in the existing proxy send handlers in `main.rs`.

**SQLite INSERT OR IGNORE pattern** — no existing analog in codebase; use RESEARCH.md Pattern 5:
```rust
// From RESEARCH.md Pattern 5 (D-03 C3 dedup fence)
fn insert_print_job(conn: &rusqlite::Connection, job_id: &str, job_type: &str)
    -> rusqlite::Result<bool>
{
    let changes = conn.execute(
        "INSERT OR IGNORE INTO printed_jobs (job_id, job_type, status, received_at)
         VALUES (?1, ?2, 'pending', datetime('now'))",
        rusqlite::params![job_id, job_type],
    )?;
    Ok(changes > 0) // true = new insert, false = duplicate
}
```

**WAL pragma** (second connection for Pusher task — RESEARCH.md Pitfall 5):
```rust
// Open a second rusqlite::Connection for the Pusher task.
// Enable WAL mode on both connections at open time.
let pusher_conn = Connection::open(&db_path)?;
pusher_conn.pragma_update(None, "journal_mode", "WAL")?;
// Also enable WAL on the main conn opened in main.rs (open_and_migrate).
```

**Dev shim pattern** (D-04, from RESEARCH.md Code Examples):
```rust
#[cfg(debug_assertions)]
fn try_fake_pusher_event(tx: &tokio::sync::mpsc::Sender<PrintEvent>) -> bool {
    let Some(raw) = std::env::var("BREVLY_FAKE_PUSHER_EVENT").ok() else { return false };
    let (job_id, job_type) = match raw.split_once(':') {
        Some((id, t)) => (id.to_string(), t.to_string()),
        None => {
            eprintln!("[brevly-print] BREVLY_FAKE_PUSHER_EVENT: invalid format");
            return false;
        }
    };
    let tx = tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let _ = tx.send(PrintEvent { job_id, job_type }).await;
    });
    true
}
```

---

### `src/noren_client.rs` — add `pusher_auth()` (service, request-response)

**Analog:** `activate()` in the same file (`src/noren_client.rs` lines 110–142) — exact same role.

**Core pattern to copy** (`src/noren_client.rs` lines 110–142):
```rust
pub async fn activate(
    client: &reqwest::Client,
    base_url: &str,
    serial: &str,
    machine_id: Option<&str>,
    force_rebind: bool,
) -> Result<ActivateResponse, ActivateError> {
    let url = format!("{base_url}/api/agent/activate");
    let resp = client
        .post(&url)
        .json(&ActivateRequest { serial, machine_id, force_rebind })
        .send()
        .await
        .map_err(ActivateError::Transport)?;
    match resp.status().as_u16() {
        200 => resp.json::<ActivateResponse>().await.map_err(ActivateError::Transport),
        403 | 404 => Err(ActivateError::InvalidSerial),
        409 => Err(ActivateError::AlreadyActiveOther),
        _ => Err(ActivateError::Transport(resp.error_for_status().unwrap_err())),
    }
}
```

**New `pusher_auth()` function** (from RESEARCH.md Pattern 6 — verified from Noren source):
```rust
// CRITICAL: param name is "channel_name" (not "channel") — Noren reads body.get('channel_name')
// CRITICAL: use .bearer_auth() — never log the token (T-02-02)
pub async fn pusher_auth(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    channel: &str,
    socket_id: &str,
) -> anyhow::Result<String> {
    #[derive(serde::Deserialize)]
    struct PusherAuthResponse { auth: String }

    let url = format!("{base_url}/api/agent/pusher/auth");
    let resp = client
        .post(&url)
        .bearer_auth(agent_token)
        .form(&[("channel_name", channel), ("socket_id", socket_id)])
        .send()
        .await
        .context("pusher_auth: HTTP transport error")?;

    match resp.status().as_u16() {
        200 => {
            let body: PusherAuthResponse = resp.json().await
                .context("pusher_auth: response parse error")?;
            Ok(body.auth)
        }
        403 => anyhow::bail!("pusher_auth: 403 — invalid token or channel mismatch"),
        status => anyhow::bail!("pusher_auth: unexpected status {status}"),
    }
}
```

Note: `pusher_auth()` uses `anyhow::Result` (not a custom error type) — consistent with the module's existing `anyhow` usage throughout `client.rs`. The inline `PusherAuthResponse` struct pattern (local Deserialize inside the fn body) matches the RESEARCH.md example and avoids polluting the module namespace.

---

### `src/main.rs` — Runtime mode spawn site (wiring)

**Analog:** Existing `main.rs` proxy wiring at lines 362–373 and the `App` struct at lines 64–84.

**`UserEvent` extension pattern** (`src/main.rs` lines 52–60):
```rust
#[derive(Debug)]
enum UserEvent {
    #[cfg(windows)]
    TrayIconEvent(tray_icon::TrayIconEvent),
    #[cfg(windows)]
    MenuEvent(tray_icon::menu::MenuEvent),
    HealthChanged(HealthState),
    // Phase 4 addition (planner decides whether to add PrintEventReceived or use standalone mpsc):
    // PrintEventReceived(PrintEvent),
}
```

**Tokio spawn pattern** (from `main.rs` context — `rt_handle` already exists, lines 302–305):
```rust
let rt = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .context("Failed to build tokio runtime")?;
let rt_handle = rt.handle().clone();
```

**Spawn pattern to replicate in Runtime mode block:**
```rust
// Phase 4: spawn Pusher reconnect loop in Runtime mode (after credential check, before event loop)
// Read Pusher credentials from ConfigStore
// Create mpsc channel for PrintEvent handoff to Phase 5
let (print_tx, print_rx) = tokio::sync::mpsc::channel::<PrintEvent>(32);
let proxy_for_pusher = event_loop.create_proxy();
let pusher_config = PusherConfig { /* read from config_store */ };
let pusher_db_path = db_path.clone();
rt_handle.spawn(async move {
    run_pusher_loop(pusher_config, agent_token, print_tx, proxy_for_pusher, pusher_db_path).await;
});
```

**`user_event()` handler extension** (`src/main.rs` lines 181–205):
```rust
fn user_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: UserEvent) {
    match event {
        // ... existing arms ...
        UserEvent::HealthChanged(state) => {
            self.health = state;
            #[cfg(windows)]
            if let Some(rt) = &self.tray_runtime {
                rt.apply_health(state);
            }
            let _ = event_loop;
        }
        // Phase 4: add PrintEventReceived arm here if using UserEvent variant approach
    }
}
```

---

### `tests/pusher_auth_test.rs` (test, request-response)

**Analog:** `tests/noren_client_test.rs` — exact same role: mock TCP stub + `#[tokio::test]` tests.

**Mock TCP stub pattern** (`tests/noren_client_test.rs` lines 17–49):
```rust
//! Integration tests for `noren_client`: status-code → ActivateError mapping.
//!
//! **Linux-testable** — uses a mock TCP listener that returns canned HTTP responses.
//! No live Noren endpoint is required.

use brevly_print::noren_client::{activate, ActivateError};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

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

**Test case pattern** (`tests/noren_client_test.rs` lines 53–73):
```rust
#[tokio::test]
async fn test_activate_200_returns_response() {
    let body = r#"{"agentToken":"tok-abc123","tenantId":"tenant-xyz",...}"#;
    let base_url = spawn_stub(200, body).await;
    let client = reqwest::Client::new();
    let result = activate(&client, &base_url, "SERIAL-001", None, false).await;
    let resp = result.expect("200 should return Ok");
    assert_eq!(resp.agent_token, "tok-abc123");
}
```

**Apply to `tests/pusher_auth_test.rs`:**
```rust
//! Integration tests for `pusher_auth()` in `noren_client`:
//! HTTP contract: 200 → Ok(auth_string), 403 → Err, transport → Err.
//!
//! Linux-testable — uses the same mock TCP stub pattern as noren_client_test.rs.

use brevly_print::noren_client::pusher_auth;
// (spawn_stub helper — copy from noren_client_test.rs)

#[tokio::test]
async fn test_pusher_auth_200_returns_auth_string() {
    let body = r#"{"auth":"key123:hmac-abc"}"#;
    let base_url = spawn_stub(200, body).await;
    let client = reqwest::Client::new();
    let result = pusher_auth(&client, &base_url, "tok-xyz",
                              "private-tenant-t1-print", "123.456").await;
    assert_eq!(result.unwrap(), "key123:hmac-abc");
}

#[tokio::test]
async fn test_pusher_auth_403_returns_err() { /* ... */ }

#[tokio::test]
async fn test_pusher_auth_connection_refused_returns_transport_err() { /* ... */ }
```

---

## Shared Patterns

### Background → Tray State via EventLoopProxy (C2 constraint)
**Source:** `src/main.rs` lines 363–373 (proxy creation) + lines 195–204 (user_event handler)
**Apply to:** `src/pusher/client.rs` (all health state transitions)

```rust
// In main.rs — proxy creation (already exists for tray/menu events):
let proxy = event_loop.create_proxy();
// Pass a clone to the Pusher task spawn:
let proxy_for_pusher = event_loop.create_proxy();

// In pusher/client.rs — send health changes (never touch tray directly):
let _ = proxy.send_event(UserEvent::HealthChanged(HealthState::Reconnecting));
let _ = proxy.send_event(UserEvent::HealthChanged(HealthState::Connected));
// `let _ =` pattern: ignore errors when event loop has exited (matches existing usage)
```

### `anyhow::Result` + `context()` Error Handling
**Source:** `src/noren_client.rs` lines 120–124 + `src/main.rs` lines 305, 312, 317
**Apply to:** `src/pusher/client.rs`, `src/noren_client.rs` (pusher_auth)

```rust
// Standard pattern:
use anyhow::Context as _;

let resp = client.post(&url).send().await.context("pusher_auth: HTTP transport error")?;
// or:
let (mut ws, _) = connect_async(&ws_url).await.context("WebSocket connect failed")?;
```

### `option_env!` for Compile-Time Config
**Source:** `src/noren_client.rs` lines 25–31
**Apply to:** Any compile-time Pusher config override (e.g., staging Pusher cluster)

```rust
pub fn noren_base_url() -> String {
    match option_env!("NOREN_BASE_URL") {
        Some(url) => url.to_string(),
        None => NOREN_BASE_URL_DEFAULT.to_string(),
    }
}
// Replicate pattern if a PUSHER_CLUSTER override is needed for staging.
```

### SQLite `rusqlite::params![]` Pattern
**Source:** `src/config_store.rs` lines 87–94
**Apply to:** `src/pusher/client.rs` (INSERT OR IGNORE in event handler)

```rust
conn.execute(
    "INSERT INTO config(key, value) VALUES(?1, ?2)
     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    rusqlite::params![key, value],
)?;
// Phase 4 mirrors this with INSERT OR IGNORE INTO printed_jobs.
```

### Module pub-use Index + `lib.rs` Registration
**Source:** `src/lib.rs` lines 1–24, `src/printer/mod.rs` lines 1–19
**Apply to:** `src/pusher/mod.rs` + `src/lib.rs`

```rust
// src/lib.rs — add after existing module declarations:
pub mod pusher;
```

### `#[cfg(debug_assertions)]` Gating
**Source:** D-04 decision (no existing codebase analog — first use of this gate in the project)
**Apply to:** `src/pusher/client.rs` (`try_fake_pusher_event`)

```rust
// Compiles out entirely in `--release` builds.
// Use at the top of the branch in run_pusher_loop():
#[cfg(debug_assertions)]
if try_fake_pusher_event(&print_tx) {
    // Idle — fake event already scheduled
    std::future::pending::<()>().await;
}
```

### Inline `#[cfg(test)] mod tests` Pattern
**Source:** `src/health_state.rs` lines 57–84
**Apply to:** `src/pusher/protocol.rs`, `src/pusher/backoff.rs`, `src/pusher/client.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name() {
        // pure function unit tests — no async, no I/O
    }
}
```

---

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| — | — | — | All files have analogs; patterns above cover every new file. The Pusher WebSocket reconnect loop has no existing WS code in the codebase, but `noren_client.rs` (async HTTP service) provides sufficient structural analog for the function/module shape. |

---

## Critical Protocol Details (not in analogs — from RESEARCH.md)

These are not patterned from existing code but must be noted for the planner:

| Detail | Value | Source |
|--------|-------|--------|
| Pusher event name | `"print:job"` (colon, not hyphen) | Verified from Noren source |
| Auth POST param name | `"channel_name"` (not `"channel"`) | Verified from Noren source + Pusher JS SDK |
| Double-decode requirement | `env.data` is a JSON-encoded string for ALL app events | Pusher protocol spec |
| WS URL format | `wss://ws-{cluster}.pusher.com/app/{key}?protocol=7&client=brevly-print&version=0.1.0` | Pusher spec |
| Channel name | `private-tenant-{tenantId}-print` | ROADMAP.md |
| WAL mode required | `pragma_update(None, "journal_mode", "WAL")` on second SQLite connection | RESEARCH.md Pitfall 5 |
| `futures-util` must be explicit dep | `futures-util = "0.3"` in `Cargo.toml` | RESEARCH.md Pitfall 6 |

---

## Metadata

**Analog search scope:** `src/` (all 17 source files), `tests/` (6 test files)
**Files scanned:** 23
**Pattern extraction date:** 2026-07-16
