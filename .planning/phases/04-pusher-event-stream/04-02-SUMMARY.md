---
phase: 04-pusher-event-stream
plan: "02"
subsystem: pusher-client
tags: [pusher, websocket, reconnect, dedup, ping-pong, mpsc, wal, tdd]
dependency_graph:
  requires: [04-01 (pusher primitives: protocol types, backoff, pusher_auth)]
  provides: [pusher::client::run_pusher_loop, C3 INSERT OR IGNORE dedup fence, Phase 4→5 mpsc seam]
  affects: [src/pusher/client.rs, src/pusher/mod.rs, src/main.rs]
tech_stack:
  added: []
  patterns: [reconnect state machine, INSERT OR IGNORE dedup fence (C3), ping/pong zombie detection (D-05), WAL second connection (Pitfall 5), debug_assertions shim (D-04), EventLoopProxy health closure (C2)]
key_files:
  created: []
  modified:
    - src/pusher/client.rs
    - src/pusher/mod.rs
    - src/main.rs
decisions:
  - "Used label break ('inner: loop) for the inner event loop to allow clean exit from tokio::select! arms"
  - "Burned the first interval tick (tick().await before select!) so the first real ping fires at 30s not t=0"
  - "Captured is_runtime bool before moving mode into App to guard the Pusher spawn block"
  - "clone()d print_tx for the spawn; original held implicitly but receiver kept in App._print_rx"
metrics:
  duration: "~5 minutes"
  completed: "2026-07-16T15:47:24Z"
  tasks_completed: 2
  files_created: 0
  files_modified: 3
---

# Phase 04 Plan 02: Pusher Client + Main Wiring Summary

**One-liner:** Full reconnecting Pusher WebSocket loop (connect → fresh auth → subscribe → dispatch → 30s ping/pong zombie → backoff) with INSERT OR IGNORE C3 dedup fence, WAL second SQLite connection, debug fake-event shim, and Runtime-mode spawn wiring in main.rs.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Build src/pusher/client.rs — reconnect loop, dedup fence, ping/pong, dev shim | 22cea0e | src/pusher/client.rs, src/pusher/mod.rs |
| 2 | Wire the Pusher task into main.rs Runtime mode | 8b9b285 | src/main.rs |

## What Was Built

### Task 1: src/pusher/client.rs

Full implementation of `pub async fn run_pusher_loop(config, agent_token, tx, send_health, db_path, http)` — a never-returning async function that:

**Connect phase (outer loop):**
1. `send_health(Reconnecting)` — tray goes yellow (D-07)
2. `connect_async(&ws_url)` — WSS to `wss://ws-{cluster}.pusher.com/app/{key}?protocol=7&client=brevly-print&version=0.1.0`
3. Reads first message — `pusher:connection_established` → `extract_socket_id()` double-decode
4. `pusher_auth(&http, &auth_url, &agent_token, &channel, &socket_id)` — FRESH per reconnect, never cached (EVT-02, Pitfall 7)
5. Sends `pusher:subscribe { channel, auth }`
6. Dedicated pre-dispatch loop waits for `pusher_internal:subscription_succeeded` before entering event dispatch (Pitfall 8)
7. `send_health(Connected)` → tray green; `attempt = 0` reset

**Inner event loop (`tokio::select!`):**
- `ws.next()` arm: dispatches `pusher:pong` (clear `awaiting_pong`), `print:job` (double-decode via `parse_print_job` + INSERT OR IGNORE + mpsc send), `pusher:error` (break), `Close`/`None`/`Err` (break), unknown events logged-and-ignored
- `ping_timer.tick()` arm: if `awaiting_pong == true` → zombie detected, break (EVT-03, D-05); else send `pusher:ping`, set `awaiting_pong = true`

**Backoff reconnect:** After inner loop breaks, `backoff_delay(attempt)` sleep then `attempt += 1` then loop back to outer loop head.

**INSERT OR IGNORE dedup fence (C3, T-04-04):**
```rust
fn insert_print_job(conn, job_id, job_type) -> rusqlite::Result<bool>
// Returns true = new insert, false = duplicate (Pusher re-delivery skips mpsc send)
```
Uses `rusqlite::params!` — no SQL injection (T-04-07).

**WAL second connection (Pitfall 5):** Opens `rusqlite::Connection::open(&db_path)` + `pragma_update(None, "journal_mode", "WAL")` at task startup. Separate from `App.conn` (not `Send`).

**Debug shim (D-04, T-04-09):** `#[cfg(debug_assertions)] fn try_fake_pusher_event(tx)` — reads `BREVLY_FAKE_PUSHER_EVENT=<jobId>:<type>`, spawns 1s-delayed mpsc send, returns `true` (caller calls `pending::<()>().await`). Compiles out entirely in `--release`.

**`src/pusher/mod.rs`:** Added `pub use client::run_pusher_loop`.

**Tests (7):** `insert_print_job_returns_true_on_first_insert`, `insert_print_job_returns_false_on_duplicate`, `insert_print_job_different_job_ids_both_insert`, `insert_print_job_writes_pending_status`, `fake_event_returns_false_when_env_not_set`, `fake_event_returns_false_on_malformed_value`, `fake_event_parses_job_id_and_type`.

### Task 2: src/main.rs Runtime Mode Wiring

Extended `main()` with:

- **Imports:** `run_pusher_loop`, `PrintEvent`, `PusherConfig`, `noren_base_url`
- **WAL on main conn:** `conn.pragma_update(None, "journal_mode", "WAL")` immediately after `open_and_migrate` (Pitfall 5 — both connections must agree on journal_mode)
- **`_print_rx` field on `App`:** `_print_rx: Option<tokio::sync::mpsc::Receiver<PrintEvent>>` holds the receiver so the mpsc channel is not dropped before Phase 5 consumes it
- **Pusher wiring (guarded by `is_runtime`):**
  - Reads `pusher_key`, `pusher_cluster`, `tenant_id`, `noren_base_url` from `config_store::get`
  - Extracts `agentToken` from `cred_result` (always `Ok` on Runtime path)
  - Creates `mpsc::channel::<PrintEvent>(32)` — receiver stored in `App._print_rx`
  - Creates health closure: `let send_health = move |state| { let _ = proxy_for_pusher.send_event(UserEvent::HealthChanged(state)); }` (C2 — never touches tray directly)
  - `rt_handle.spawn(async move { run_pusher_loop(...).await })` on existing runtime

## Verification Results

```
cargo build --lib   → Finished (0 errors, 0 warnings)
cargo build         → Finished (0 errors, 0 warnings)
cargo test --lib -- pusher::client::tests
                    → 7 passed
cargo test          → 34 passed, 1 ignored (10 suites)
```

All plan acceptance criteria satisfied:
- `run_pusher_loop` implements connect → fresh auth → subscribe → dispatch → ping/pong zombie → backoff
- `INSERT OR IGNORE` dedup fence gates mpsc send (C3, T-04-04)
- Event matched as `"print:job"` (colon — Pitfall 1); double-decode via `parse_print_job`
- Auth re-requested on every reconnect with new socket_id (EVT-02, T-04-05, Pitfall 7)
- One missed pong → reconnect with exponential backoff (EVT-03, D-05)
- Tray driven only via EventLoopProxy closure (C2, T-04-08); failures stay yellow never red (D-07)
- Second WAL SQLite connection in Pusher task; main conn also WAL (Pitfall 5)
- Debug-only fake shim compiles out in `--release` (T-04-09)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `std::env::set_var`/`remove_var` require unsafe in Rust 1.81**
- **Found during:** Task 1 — `cargo test --lib -- pusher::client::tests` compile error
- **Issue:** Rust 1.81 marked `std::env::set_var` and `std::env::remove_var` as `unsafe` functions. The test code used them without `unsafe {}` blocks.
- **Fix:** Wrapped each call in `unsafe {}` with a SAFETY comment explaining single-threaded test process and no concurrent env access.
- **Files modified:** `src/pusher/client.rs`
- **Commit:** 22cea0e (same task commit)

## Known Stubs

- **`App._print_rx`**: Holds the `mpsc::Receiver<PrintEvent>` for Phase 5. This is intentional — the receiver must remain alive so the channel is not dropped. Phase 5 will take it out of `Option`. Not a data stub; no hardcoded values flow to the UI.

## Threat Flags

No new threat surface introduced beyond the plan's `<threat_model>`. All T-04-04 through T-04-09 mitigations were implemented:

| Threat ID | Mitigation Implemented |
|-----------|----------------------|
| T-04-04 | `INSERT OR IGNORE` + `changes()==0` guard on mpsc send |
| T-04-05 | Auth string never stored; fresh `pusher_auth()` per reconnect |
| T-04-06 | 30s ping/pong; one missed pong → reconnect with backoff |
| T-04-07 | `rusqlite::params!` binding — no string concatenation in SQL |
| T-04-08 | Health via `proxy_for_pusher.send_event`; Pusher task never imports tray-icon |
| T-04-09 | `#[cfg(debug_assertions)]` on shim + `try_fake_pusher_event` |

## Self-Check: PASSED
