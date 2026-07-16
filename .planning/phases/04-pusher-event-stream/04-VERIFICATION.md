---
phase: 04-pusher-event-stream
verified: 2026-07-16T16:30:00Z
status: human_needed
score: 8/8 must-haves verified
overrides_applied: 0
human_verification:
  - test: "Tray turns green after subscription_succeeded"
    expected: "On a machine with a real (or test) Pusher app + Noren auth endpoint live, launch the agent in Runtime mode; tray icon transitions from yellow (Reconnecting) to green (Connected) after subscription_succeeded arrives"
    why_human: "Requires a live Pusher connection + Noren /api/agent/pusher/auth endpoint; cannot be verified with grep or unit tests"
  - test: "BREVLY_FAKE_PUSHER_EVENT=test123:order enqueues event within 500 ms"
    expected: "A debug build with the env var set bypasses the real WebSocket, emits the synthetic PrintEvent after ~1s delay, and Phase 5 worker slot receives it. Tray transitions to Connected via send_health."
    why_human: "Requires a running binary with debug assertions; the unit tests cover the shim parse logic but not the end-to-end flow through mpsc to the Phase 5 receiver"
  - test: "Zombie connection reconnects with exponential backoff (EVT-03)"
    expected: "Disable network for > 30s while connected; agent detects the missed pong, reconnects (tray goes yellow), then goes green again when network is restored. No events missed."
    why_human: "Requires a live Pusher connection and network manipulation; cannot be automated without a full integration test environment"
---

# Phase 4: Pusher Event Stream Verification Report

**Phase Goal:** The agent subscribes to its tenant's private Pusher channel, receives print-event notifications reliably, and recovers from network interruptions without missing events
**Verified:** 2026-07-16T16:30:00Z
**Status:** human_needed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `pusher_auth()` returns Ok(auth_string) on 200 and Err on 403/transport | VERIFIED | `src/noren_client.rs:177-219` — `pub async fn pusher_auth` maps 200→Ok(body.auth), 403→bail!, other→bail!; 3 integration tests pass (`cargo test --test pusher_auth_test`: 3 passed) |
| 2 | `parse_print_job()` double-decodes the JSON-in-JSON data field into {jobId, type} | VERIFIED | `src/pusher/protocol.rs:68-82` — matches `Value::String(s)`, calls `from_str::<PrintEvent>(s)`; returns Err on non-string data; inline tests pass (12 lib tests passed) |
| 3 | `extract_socket_id()` reads socket_id from the connection_established envelope | VERIFIED | `src/pusher/protocol.rs:90-101` — `env.data.as_str()`, parses `ConnectionEstablishedData { socket_id }`, returns it; inline test `extract_socket_id_parses_connection_established` passes |
| 4 | `backoff_delay(attempt)` never exceeds the 60s cap for any attempt | VERIFIED | `src/pusher/backoff.rs:35-44` — base capped at 60_000 before jitter, final result also `.min(60_000)`; test `backoff_never_exceeds_60s_cap` runs attempts 0..=20 all asserting <= 60_000ms, passes |
| 5 | Activation save persists pusher_key and pusher_cluster to ConfigStore (D-01) | VERIFIED | `src/activation_window.rs:977-987` — `("pusher_key", pusher_key)` and `("pusher_cluster", pusher_cluster)` tuples added to `config_entries` vec, sourced from `state.pusher_key` / `state.pusher_cluster` |
| 6 | On event receipt the agent INSERT OR IGNOREs the job into printed_jobs then mpsc-sends only new (non-duplicate) events to the print worker | VERIFIED | `src/pusher/client.rs:59-71` — `INSERT OR IGNORE INTO printed_jobs` with `rusqlite::params!`; returns `changes > 0`; line 300-309: only calls `tx.send(event).await` when `Ok(true)`; tests `insert_print_job_returns_true_on_first_insert` and `insert_print_job_returns_false_on_duplicate` pass |
| 7 | On each connect the agent extracts a fresh socket_id and calls pusher_auth() — the auth string is never cached across reconnects (EVT-02) | VERIFIED | `src/pusher/client.rs:206` — `let auth = match pusher_auth(...)` is created as a local variable within the outer reconnect loop, not stored on any struct field; every iteration of the outer `loop` re-runs the auth POST with the new socket_id |
| 8 | One missed pong within the 30s ping window declares the connection zombie and triggers reconnect with exponential backoff (EVT-03) | VERIFIED | `src/pusher/client.rs:270-355` — `let mut ping_timer = interval(30s)`; first tick burned at line 272; `awaiting_pong` flag; on `ping_timer.tick()` at line 343: if `awaiting_pong` → break inner loop; outer loop calls `backoff_delay(attempt)` + `attempt += 1` |

**Score:** 8/8 truths verified

### Deferred Items

None — all must-haves are fully addressed in this phase.

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/pusher/protocol.rs` | PusherEnvelope, PrintEvent, PusherConfig, parse_print_job, extract_socket_id | VERIFIED | File exists, 151 lines, all 5 required symbols present with inline tests |
| `src/pusher/backoff.rs` | backoff_delay with jitter, 60s cap | VERIFIED | File exists, 73 lines, `pub fn backoff_delay` with ±25% jitter and dual cap |
| `src/pusher/mod.rs` | pusher module index with pub mod protocol, pub mod backoff, pub use run_pusher_loop | VERIFIED | File exists; `pub mod backoff`, `pub mod client`, `pub mod protocol`; `pub use client::run_pusher_loop`; `pub use protocol::{PrintEvent, PusherConfig}` |
| `src/pusher/client.rs` | run_pusher_loop reconnect loop, insert_print_job dedup, dev shim | VERIFIED | File exists, 488 lines, substantive implementation; `pub async fn run_pusher_loop`, `fn insert_print_job`, `#[cfg(debug_assertions)] fn try_fake_pusher_event` |
| `src/noren_client.rs` | pusher_auth() channel-auth POST | VERIFIED | `pub async fn pusher_auth` at line 177; uses `bearer_auth`, `channel_name` form field, maps 200/403/other |
| `tests/pusher_auth_test.rs` | HTTP contract tests for pusher_auth (EVT-01, EVT-02) | VERIFIED | File exists, 3 `#[tokio::test]` functions testing 200/403/transport; all 3 pass |
| `src/main.rs` (Runtime wiring) | run_pusher_loop spawn, config from ConfigStore, mpsc channel, health closure | VERIFIED | Lines 389-452; reads `pusher_key`, `pusher_cluster`, `tenant_id`, `noren_base_url`; creates `mpsc::channel::<PrintEvent>(32)`; health closure via `proxy_for_pusher.send_event(UserEvent::HealthChanged(state))`; `rt_handle.spawn(async move { run_pusher_loop(...).await })` |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/lib.rs` | `src/pusher/mod.rs` | `pub mod pusher` | WIRED | `src/lib.rs:16` — `pub mod pusher;` present |
| `src/activation_window.rs` | `config_store::set` | pusher_key / pusher_cluster entries | WIRED | Lines 986-987 — `("pusher_key", pusher_key)` and `("pusher_cluster", pusher_cluster)` in `config_entries` vec, passed to `config_store::set` |
| `src/pusher/client.rs` | `printed_jobs` table | INSERT OR IGNORE | WIRED | Line 65-67 — `INSERT OR IGNORE INTO printed_jobs (job_id, job_type, status, received_at) VALUES (?1, ?2, 'pending', datetime('now'))` |
| `src/pusher/client.rs` | `noren_client::pusher_auth` | fresh auth POST per connect | WIRED | Line 39 — `use crate::noren_client::pusher_auth;`; called at line 206 inside the outer reconnect loop |
| `src/pusher/client.rs` | `UserEvent::HealthChanged` | `send_health` closure | WIRED | `run_pusher_loop` accepts `send_health: impl Fn(HealthState) + Send + 'static`; `main.rs:428-430` wires it to `proxy_for_pusher.send_event(UserEvent::HealthChanged(state))` |
| `src/main.rs` | `run_pusher_loop` | `rt_handle.spawn` in Runtime mode | WIRED | Lines 435-437 — `rt_handle.spawn(async move { run_pusher_loop(...).await })` inside `if is_runtime { ... }` |
| `src/pusher/mod.rs` | `client::run_pusher_loop` | `pub use client::run_pusher_loop` | WIRED | `src/pusher/mod.rs:10` |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `src/pusher/client.rs` | `event: PrintEvent` | WebSocket `ws.next()` → `parse_print_job(&env)` → `insert_print_job` → `tx.send(event)` | Yes — from live Pusher WS stream or debug shim | FLOWING |
| `src/activation_window.rs` | `pusher_key`, `pusher_cluster` | `state.pusher_key` / `state.pusher_cluster` from `ActivateResponse`, persisted via `config_store::set` | Yes — from Noren `/api/agent/activate` response | FLOWING |
| `src/main.rs` | `pusher_key`, `pusher_cluster` | `config_store::get(&conn, "pusher_key")` / `config_store::get(&conn, "pusher_cluster")` | Yes — reads from SQLite ConfigStore written at activation | FLOWING |
| `src/main.rs` | `_print_rx` | `print_rx` receiver from `mpsc::channel::<PrintEvent>(32)` | Yes — held alive in `App._print_rx: Option<...>` for Phase 5 to consume | FLOWING (seam preserved) |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| lib tests including all pusher inline tests | `cargo test --lib -- pusher::` | 12 passed, 4 filtered out (1.00s) | PASS |
| pusher_auth HTTP contract tests | `cargo test --test pusher_auth_test` | 3 passed (0.04s) | PASS |
| Full test suite (no regressions) | `cargo test` | 34 passed, 1 ignored (10 suites, 1.08s) | PASS |
| lib builds cleanly | `cargo build --lib` | Finished `dev` profile (0 errors) | PASS |

### Probe Execution

No probe scripts found (`scripts/` directory does not exist). Phase declares manual smoke tests in `04-02-PLAN.md` `<verification>` section as deferred to UAT. Step 7b: SKIPPED for live Pusher smoke checks (require running binary + real network); covered by human verification items below.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| EVT-01 | 04-01-PLAN.md, 04-02-PLAN.md | Agent connects to private Pusher channel and receives `{jobId, type}` print events | SATISFIED | `run_pusher_loop` connects, subscribes, dispatches `print:job` events via `parse_print_job` (double-decode) + `tx.send`; `pusher_auth_test` covers auth contract; `protocol.rs` inline tests cover double-decode |
| EVT-02 | 04-01-PLAN.md, 04-02-PLAN.md | Channel auth re-done on every reconnect with fresh `socket_id` — never reuse cached auth | SATISFIED | `let auth = pusher_auth(...)` is a local variable in the outer reconnect loop (line 206); no struct field caches it; confirmed no `let mut auth` or struct-stored auth across iterations |
| EVT-03 | 04-01-PLAN.md, 04-02-PLAN.md | Ping/pong zombie detection + exponential backoff reconnect | SATISFIED | `ping_timer` every 30s; `awaiting_pong` flag; missed pong breaks inner loop; `backoff_delay(attempt)` sleep; `attempt += 1`; `backoff_delay` cap verified by unit test |

All 3 requirements for Phase 4 are SATISFIED by implementation evidence.

### Anti-Patterns Found

No debt markers (TBD, FIXME, XXX) found in any phase-modified file.
No warning-level markers (TODO, HACK, PLACEHOLDER) found.
No empty return stubs (return null, return {}, return []) found.

The `App._print_rx: Option<tokio::sync::mpsc::Receiver<PrintEvent>>` is an intentional Phase 4→5 handoff seam, not a data stub — its purpose is to keep the channel alive until Phase 5 takes ownership. No hardcoded empty values flow to any UI or rendering path.

### Human Verification Required

#### 1. Tray Turns Green After subscription_succeeded

**Test:** On a Windows machine with the agent compiled in release mode, configure a real Pusher test app (or connect to Noren's staging Pusher app). Activate the agent with valid credentials. Observe the tray icon.
**Expected:** Tray icon transitions yellow (Reconnecting) → green (Connected) after `pusher_internal:subscription_succeeded` is received. This confirms the `send_health(HealthState::Connected)` → `EventLoopProxy` → `App::user_event` → `TrayRuntime::apply_health` path is live.
**Why human:** Requires a live Pusher WebSocket connection and a running Windows binary; cannot be verified with grep or automated tests.

#### 2. BREVLY_FAKE_PUSHER_EVENT Injects Event Through mpsc (D-04)

**Test:** Build a debug binary (`cargo build`). Set `BREVLY_FAKE_PUSHER_EVENT=test123:order` in the environment. Launch the agent in Runtime mode (already activated). Observe that after ~1 second the PrintEvent is received by the Phase 5 mpsc receiver slot.
**Expected:** The debug shim fires within 1s, the health transitions to Connected (green tray), and the mpsc receiver gets `PrintEvent { job_id: "test123", job_type: "order" }`.
**Why human:** The unit tests cover the shim's parse and send logic, but end-to-end flow through the running binary (runtime init → fake shim → mpsc → Phase 5 receiver slot) requires a running process.

#### 3. Zombie Connection Detected and Reconnected (EVT-03)

**Test:** With the agent running and connected (green tray), block network traffic for > 30 seconds. Observe tray during and after the block. Restore network.
**Expected:** After ~30s with no pong response, the agent breaks the inner loop, transitions to yellow (Reconnecting), then reconnects with exponential backoff, re-authenticates with the new socket_id, re-subscribes, and transitions back to green. No events missed (any events emitted by Noren during outage are recovered via the server-side queue in Phase 6).
**Why human:** Requires live Pusher connection + network manipulation (firewall rule or physical disconnect); cannot be simulated in automated tests without a mock WebSocket server.

### Gaps Summary

No gaps identified. All 8 must-have truths are VERIFIED against codebase evidence, all artifacts exist and are substantive, all key links are wired, all requirements (EVT-01, EVT-02, EVT-03) are satisfied by implementation, the full test suite is green (34 passed, 1 ignored), and no anti-patterns or debt markers were found.

Status is `human_needed` because the phase goal ("subscribes to its private Pusher channel, receives print-event notifications reliably, and recovers from network interruptions without missing events") has three success criteria (SC-1, SC-2/D-04, SC-3) that require a live environment to confirm the real-time behavior of the running agent.

---

_Verified: 2026-07-16T16:30:00Z_
_Verifier: Claude (gsd-verifier)_
