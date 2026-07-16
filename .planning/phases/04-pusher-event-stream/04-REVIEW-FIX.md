---
phase: 04-pusher-event-stream
fixed_at: 2026-07-16T00:00:00Z
review_path: .planning/phases/04-pusher-event-stream/04-REVIEW.md
iteration: 1
findings_in_scope: 10
fixed: 10
skipped: 0
status: all_fixed
---

# Phase 04: Code Review Fix Report

**Fixed at:** 2026-07-16
**Source review:** .planning/phases/04-pusher-event-stream/04-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 10 (5 Critical + 5 Warning; Info items excluded per fix_scope)
- Fixed: 10
- Skipped: 0

## Fixed Issues

### CR-01: First WebSocket frame is passed to `extract_socket_id` without verifying the event name

**Files modified:** `src/pusher/client.rs`
**Commit:** 8121c85
**Applied fix:** Expanded the Step 3 match arm to first parse the envelope, then check `env.event == "pusher:connection_established"` before calling `extract_socket_id`. If the event is something else (e.g. `pusher:error`), the event name and data are logged and the reconnect loop continues with backoff.

---

### CR-02: Original `print_tx` sender is never dropped — Phase 5 receiver cannot detect Pusher task death

**Files modified:** `src/main.rs`
**Commit:** e8fdaec
**Applied fix:** Added `drop(print_tx)` immediately after the `rt_handle.spawn(...)` call so that only the `pusher_tx` clone (moved into the task) keeps the channel open. When the Pusher task exits, `rx.recv()` will return `None` as expected by Phase 5.

---

### CR-03: `hmac` and `sha2` are declared as Phase 4 auth dependencies but are never used

**Files modified:** `Cargo.toml`, `src/noren_client.rs`
**Commit:** 670e9e8
**Applied fix:** Removed the `hmac = "0.13"` and `sha2 = "0.11"` entries (plus their comment) from `Cargo.toml`. Added a doc comment above `pusher_auth()` in `noren_client.rs` explicitly documenting that auth is server-delegated: the Noren backend holds the Pusher app secret and returns a pre-signed `"app_key:hmac_sha256"` string.

---

### CR-04: Stale agent-token can be persisted after the user edits the serial field post-activation

**Files modified:** `src/activation_state.rs`
**Commit:** b0a9586
**Applied fix:** Extended `on_serial_changed()` to clear `agent_token`, `tenant_id`, `pusher_key`, `pusher_cluster`, `enabled_types`, `flow`, and `test_print_confirmed` whenever the user edits the serial after a successful activation. The guard `if self.agent_token.is_some()` ensures this only fires when there is a stale token to clear.

---

### CR-05: `unsafe` env-var mutation in tests is unsound under the multi-threaded test runner

**Files modified:** `src/pusher/client.rs`
**Commit:** 80cbf21
**Applied fix:** Refactored `try_fake_pusher_event` to accept `override_val: Option<&str>`. When `Some(value)` is passed the function uses it directly; when `None` is passed it reads from `BREVLY_FAKE_PUSHER_EVENT`. The call site in `run_pusher_loop` passes `None`. All three tests were rewritten to pass explicit `Some(...)` or `None` values, eliminating all `unsafe` env mutation.

---

### WR-01: Empty Pusher credentials from ConfigStore produce unroutable URLs with no diagnostic

**Files modified:** `src/main.rs`
**Commit:** 469eaf5
**Applied fix:** Replaced `.unwrap_or_default()` for `pusher_key`, `pusher_cluster`, and `tenant_id` with `.filter(|s| !s.is_empty()).context("... missing from ConfigStore — re-activate to restore")?`. Missing or empty credentials now produce an immediate fatal error with a clear message rather than silently constructing an invalid WebSocket URL. (`noren_base_url` retains its `unwrap_or_else` fallback since it has a valid default.)

---

### WR-02: Unreachable `Err(_) => String::new()` fallback silently produces an empty agent token

**Files modified:** `src/main.rs`
**Commit:** 0fb8d62
**Applied fix:** Replaced `Err(_) => String::new(), // unreachable` with `Err(e) => unreachable!("Runtime path requires Ok credential, but got: {e}")`. If the Runtime/Activation logic ever diverges, this now panics loudly instead of silently producing an empty bearer token.

---

### WR-03: `pusher:error` event data is never logged — Pusher error codes are invisible to operators

**Files modified:** `src/pusher/client.rs`
**Commit:** 6c70914
**Applied fix:** Both `pusher:error` handlers now include `env.data` in the `eprintln!` message. The subscription-wait handler logs "error before subscription_succeeded — reconnecting. Details: {data}" and the inner dispatch handler logs "received pusher:error — reconnecting. Details: {data}". Operators can now see the Pusher error code (e.g. `{"code":4001,"message":"App key not valid"}`).

---

### WR-04: `tokio::time::interval` uses `MissedTickBehavior::Burst` — spurious reconnect on system sleep/wake

**Files modified:** `src/pusher/client.rs`
**Commit:** 9d9588b
**Applied fix:** Added `ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay)` immediately after creating the ping interval. This prevents sleep/wake from firing multiple ticks in rapid succession and triggering a false zombie-reconnect.

---

### WR-05: `resp.error_for_status().unwrap_err()` panics if server returns an unexpected 2xx status

**Files modified:** `src/noren_client.rs`
**Commit:** 5ab3c25
**Applied fix:** Added an explicit `201..=299` arm before the catch-all `_` arm. Unexpected 2xx responses are handled by attempting to deserialize the body as `ActivateResponse`, which produces a `reqwest::Error` (JSON parse failure) without calling `unwrap_err()`. The original `_ =>` arm — now only reached by non-2xx, non-handled status codes — retains `error_for_status().unwrap_err()` which is safe there since those codes are guaranteed to produce `Err`.

---

_Fixed: 2026-07-16_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
