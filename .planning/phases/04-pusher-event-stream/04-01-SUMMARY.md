---
phase: 04-pusher-event-stream
plan: "01"
subsystem: pusher-primitives
tags: [pusher, websocket, backoff, protocol, config-persistence, tdd]
dependency_graph:
  requires: [02-activation (config_store pusher_key/pusher_cluster persistence)]
  provides: [pusher::protocol (PrintEvent, PusherConfig, parse_print_job, extract_socket_id), pusher::backoff (backoff_delay), noren_client::pusher_auth]
  affects: [src/pusher/*, src/noren_client.rs, src/activation_window.rs, src/lib.rs]
tech_stack:
  added: [futures-util 0.3, rand 0.8]
  patterns: [double-decode (Pitfall 3 defense), jittered exponential backoff (D-06), bearer_auth token never logged (T-04-01), channel_name form field (Pitfall 2 defense)]
key_files:
  created:
    - src/pusher/mod.rs
    - src/pusher/protocol.rs
    - src/pusher/backoff.rs
    - src/pusher/client.rs
    - tests/pusher_auth_test.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/lib.rs
    - src/noren_client.rs
    - src/activation_window.rs
decisions:
  - "Used rand::random::<f64>() for jitter — cleaner than SystemTime modulo arithmetic (resolves RESEARCH.md Open Question 2)"
  - "Created empty src/pusher/client.rs placeholder so pub mod client; compiles across waves without Plan 02 content"
  - "Added reqwest form feature (Rule 1 bug fix — form feature was missing from reqwest dependency declaration)"
metrics:
  duration: "~7 minutes"
  completed: "2026-07-16T15:37:32Z"
  tasks_completed: 3
  files_created: 5
  files_modified: 5
---

# Phase 04 Plan 01: Pusher Primitives Summary

**One-liner:** Pusher protocol types and parsers (double-decode), exponential backoff with jitter, and pusher_auth HTTP POST — the tested contracts Plan 02's reconnect loop builds against.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Add futures-util + rand deps and persist pusher_key/pusher_cluster (D-01) | 044d96c | Cargo.toml, Cargo.lock, src/activation_window.rs |
| 2 | Pusher protocol types + parsers and backoff, with inline tests | 04d449e | src/pusher/{mod,protocol,backoff,client}.rs, src/lib.rs |
| 3 | Add pusher_auth() to noren_client with HTTP-contract tests (EVT-01, EVT-02) | ef3312c | src/noren_client.rs, tests/pusher_auth_test.rs, Cargo.toml, Cargo.lock |

## What Was Built

### Task 1: Dependencies + D-01 Fence

`futures-util = "0.3"` and `rand = "0.8"` added to `[dependencies]` (portable section, not
Windows-only). The activation save loop in `activation_window.rs` now persists `pusher_key`
and `pusher_cluster` to ConfigStore alongside the existing fields. This is the D-01 fence:
without it, Plan 02's Runtime-mode startup cannot read the Pusher app key/cluster and the
WebSocket connection has no credentials.

### Task 2: Pusher Protocol Module

New `src/pusher/` module with three files:

- **`protocol.rs`**: `PusherEnvelope`, `PrintEvent` (with `jobId`/`type` rename), `PusherConfig`
  (key/cluster/tenant_id/auth_url bundle). `parse_print_job()` implements the double-decode
  contract: `data` must be a `Value::String`; non-string variants return `Err` (Pitfall 3
  defense). `extract_socket_id()` parses the `connection_established` inner JSON. Five inline
  unit tests cover all five behaviors from the plan.

- **`backoff.rs`**: `backoff_delay(attempt)` — base `1000ms * 2^attempt.min(6)`, capped at
  60 000 ms, jittered ±25% via `rand::random::<f64>()`, final result also capped at 60 000 ms
  (jitter never exceeds ceiling per D-06). Two inline tests: cap for attempts 0..=20 and
  attempt-0 jitter range [750ms, 1250ms].

- **`mod.rs`**: flat pub-use index; `pub mod client` placeholder (Plan 02 fills it);
  re-exports `PrintEvent` and `PusherConfig`.

- **`client.rs`**: empty placeholder with doc comment so `pub mod client;` compiles.

`src/lib.rs` gains `pub mod pusher;`.

### Task 3: pusher_auth() + HTTP Tests

`noren_client.rs` gains `pub async fn pusher_auth()` which POSTs to `/api/agent/pusher/auth`
with `.bearer_auth(agent_token)` and form body `channel_name=<channel>&socket_id=<socket_id>`.
The field name is `channel_name` (not `channel`) — Noren reads `body.get('channel_name')`,
verified from Noren source (Pitfall 2). Token is never formatted into logs (T-04-01). Status
mapping: 200 → `Ok(body.auth)`, 403 → `Err`, other/transport → `Err`.

`tests/pusher_auth_test.rs` has three `#[tokio::test]` cases against a mock TCP stub:
1. 200 with `{"auth":"key123:hmac-abc"}` → `Ok("key123:hmac-abc")`
2. 403 → `Err` (message contains "403" but not the token)
3. Connection-refused → `Err` (transport error)

## Verification Results

```
cargo build --lib                          → Finished (0 errors)
cargo test --lib -- pusher::               → 5 passed
cargo test --test pusher_auth_test         → 3 passed
cargo test                                 → 27 passed, 1 ignored (0 failures)
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Missing `form` feature in reqwest dependency**
- **Found during:** Task 3 — `cargo test --test pusher_auth_test` compile error
- **Issue:** `reqwest` was declared with `features = ["rustls", "json"]` but the `.form()`
  method requires the `"form"` feature to be enabled. The compile error was:
  `no method named 'form' found for struct RequestBuilder in the current scope`
- **Fix:** Added `"form"` to reqwest's features list in Cargo.toml
- **Files modified:** `Cargo.toml`, `Cargo.lock`
- **Commit:** ef3312c (same task commit)

**2. [Rule 2 - Auto-added] Security assertion in 403 error test**
- **Found during:** Task 3 test writing
- **Issue:** T-04-01 requires the agent token never appear in logs or error messages.
  The test asserts both `err_msg.contains("403")` AND `!err_msg.contains("tok-invalid")`.
  The plan only specified "403 returns Err" without the negative token-leak check.
- **Fix:** Added `assert!(!err_msg.contains("tok-invalid"), ...)` to the 403 test.
- **Files modified:** `tests/pusher_auth_test.rs`

## Known Stubs

- **`src/pusher/client.rs`**: Empty placeholder — doc comment only. This is intentional:
  Plan 01 creates the placeholder so `pub mod client;` in `mod.rs` compiles. Plan 02 fills
  this with `run_pusher_loop()`. Not a data stub — no hardcoded values flow to UI rendering.

## Threat Flags

No new threat surface introduced beyond what is documented in the plan's `<threat_model>`.
All T-04-0x mitigations were implemented:
- T-04-01: `bearer_auth()` used; token never string-formatted into any log/error message (verified by test)
- T-04-02: 403 surfaces as `Err`; no retry on mismatch (reconnect handles the outer loop in Plan 02)
- T-04-03: `serde_json` typed structs; non-string `data` in `parse_print_job` returns `Err`
- T-04-SC: `futures-util` and `rand` were pre-audited [OK] in RESEARCH.md

## Self-Check: PASSED
