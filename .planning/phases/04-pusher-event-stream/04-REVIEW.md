---
phase: 04-pusher-event-stream
reviewed: 2026-07-16T16:05:02Z
depth: standard
files_reviewed: 9
files_reviewed_list:
  - src/activation_window.rs
  - src/lib.rs
  - src/main.rs
  - src/noren_client.rs
  - src/pusher/backoff.rs
  - src/pusher/client.rs
  - src/pusher/mod.rs
  - src/pusher/protocol.rs
  - tests/pusher_auth_test.rs
findings:
  critical: 5
  warning: 5
  info: 3
  total: 13
status: issues_found
---

# Phase 04: Code Review Report

**Reviewed:** 2026-07-16T16:05:02Z
**Depth:** standard
**Files Reviewed:** 9
**Status:** issues_found

## Summary

Reviewed the Phase 4 Pusher event stream implementation across the full file scope.
The reconnect loop architecture is sound — the C3 dedup fence, EVT-02 (fresh auth per
reconnect), zombie-detection ping/pong, and health-state-via-proxy pattern are all
correctly structured. The protocol double-decode and the backoff cap are correct.

Five critical issues were found:

1. The first WebSocket frame is passed to `extract_socket_id` without verifying it is
   `pusher:connection_established`, allowing a `pusher:error` or unrelated frame to be
   misread and producing misleading diagnostics.
2. The original `print_tx` sender is never dropped after spawning the Pusher task,
   making it impossible for the Phase 5 receiver to detect Pusher task death via
   channel closure.
3. `hmac` and `sha2` crates are declared as dependencies (with a Phase 4 auth comment)
   but are never imported or used — the auth design is server-delegated, which
   contradicts what the CLAUDE.md stack document describes.
4. `activation_window.rs` has a stale-token save bug: editing the serial field after a
   successful activation does not clear `agent_token`, so pressing Enter or "Salvar"
   persists the token from the previous serial.
5. `std::env::set_var`/`remove_var` are called in tests under `unsafe` with a
   "single-threaded" comment that is incorrect — the default test runner is
   multi-threaded, making these tests unsound.

---

## Narrative Findings (AI reviewer)

## Critical Issues

### CR-01: First WebSocket frame is passed to `extract_socket_id` without verifying the event name

**File:** `src/pusher/client.rs:183-203`
**Issue:** Step 3 reads the first frame from the server and calls
`parse_envelope(...).and_then(|env| extract_socket_id(&env))` without first checking
that `env.event == "pusher:connection_established"`. Any frame type that happens to
be a valid text frame is fed to `extract_socket_id`, which only fails if `env.data`
is not a string.

The most concrete failure path is a `pusher:error` before connection is established
(e.g. invalid app key, rate limit). Pusher encodes the error `data` as a JSON object
`{"code":4001,"message":"App key not valid"}`, not as a string. `extract_socket_id`
calls `.data.as_str()` on an Object variant, returns `Err`, and the outer match falls
into the error branch — but the log says "failed to extract socket_id" with no
indication of the actual Pusher error code. The tray enters infinite reconnect with
misleading diagnostics.

A second path: if a `pusher:ping` or any other string-data frame arrives first, its
string `data` field is passed to `ConnectionEstablishedData` deserialization, which
fails because there is no `socket_id` field — again falling through with misleading
messages.

**Fix:**
```rust
// Step 3: expect pusher:connection_established, extract socket_id
let socket_id = match ws.next().await {
    Some(Ok(Message::Text(text))) => {
        let env = match parse_envelope(text.as_str()) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[brevly-print] Pusher: failed to parse first frame: {e:#}");
                tokio::time::sleep(backoff_delay(attempt)).await;
                attempt = attempt.saturating_add(1);
                continue;
            }
        };
        if env.event != "pusher:connection_established" {
            eprintln!(
                "[brevly-print] Pusher: expected connection_established, got '{}' \
                 (data: {})",
                env.event, env.data
            );
            tokio::time::sleep(backoff_delay(attempt)).await;
            attempt = attempt.saturating_add(1);
            continue;
        }
        match extract_socket_id(&env) {
            Ok(id) => id,
            Err(e) => {
                eprintln!("[brevly-print] Pusher: failed to extract socket_id: {e:#}");
                tokio::time::sleep(backoff_delay(attempt)).await;
                attempt = attempt.saturating_add(1);
                continue;
            }
        }
    }
    other => {
        eprintln!("[brevly-print] Pusher: unexpected first message: {other:?}");
        tokio::time::sleep(backoff_delay(attempt)).await;
        attempt = attempt.saturating_add(1);
        continue;
    }
};
```

---

### CR-02: Original `print_tx` sender is never dropped — Phase 5 receiver cannot detect Pusher task death

**File:** `src/main.rs:399-451`
**Issue:** The channel is created at line 399 with two ends, `print_tx` and `print_rx`.
A clone of the sender (`pusher_tx`) is moved into the Pusher task at line 434-437.
The **original `print_tx`** is never moved into a struct, never explicitly dropped, and
never consumed — it stays alive in local scope until `main()` returns (after
`event_loop.run_app()`).

`_print_rx` is stored in `App` to prevent the receiver from being dropped prematurely
(correctly noted in the comment). But the channel semantics require **all senders** to
be dropped before `rx.recv()` returns `None`. Because `print_tx` remains alive
indefinitely alongside the event loop, `recv()` on `_print_rx` will block forever even
after the Pusher task exits or panics.

Phase 5 will call `self._print_rx.take()` and poll it. If the Pusher task crashes, the
Phase 5 worker will hang waiting on a channel that will never close, because the stray
`print_tx` in the main scope keeps it open.

**Fix:** Explicitly drop the original sender immediately after the Pusher spawn so that
only the Pusher task's `pusher_tx` clone counts:
```rust
let pusher_tx = print_tx.clone();
rt_handle.spawn(async move {
    run_pusher_loop(pusher_config, agent_token, pusher_tx, send_health,
                   pusher_db_path, pusher_http).await;
});
// Drop original sender — only pusher_tx (now moved into the task) keeps the
// channel open. When the Pusher task exits, rx.recv() will return None.
drop(print_tx);
```

---

### CR-03: `hmac` and `sha2` are declared as Phase 4 auth dependencies but are never used — auth model is undocumented

**File:** `Cargo.toml:46-47`
**Issue:** The dependency section reads:
```toml
# Pusher HMAC auth (Phase 4)
hmac = "0.13"
sha2 = "0.11"
```
A full-codebase search (`grep -r "hmac::\|sha2::" src/`) finds zero uses of either
crate. Pusher channel auth is delegated entirely to the Noren backend:
`noren_client::pusher_auth()` POSTs `channel_name` + `socket_id` to
`/api/agent/pusher/auth` and receives a pre-computed `"key:hmac"` string. The agent
never signs anything locally.

This is a blocker for two reasons:
1. The CLAUDE.md tech-stack document lists `hmac` + `sha2` as "HMAC-SHA256 for Pusher
   auth signature", establishing the expectation of local signing. If that was removed
   in favour of server delegation, the decision is not documented in code or in the
   plan, and the dependencies are dead weight (increased binary size, expanded
   supply-chain attack surface).
2. If local HMAC verification was required (e.g., verifying the returned auth token
   format), the implementation is silently absent.

**Fix:** Remove both crates from `Cargo.toml` if server-delegated auth is the final
design:
```toml
# Remove:
# hmac = "0.13"
# sha2 = "0.11"
```
Add a comment near `pusher_auth()` in `noren_client.rs` explicitly documenting why
local HMAC is not performed:
```rust
// Auth is delegated to the Noren backend: we POST (channel_name, socket_id) and
// receive a pre-signed "app_key:hmac_sha256" string. No local HMAC is computed;
// the backend holds the Pusher app_secret.
```

---

### CR-04: Stale agent-token can be persisted after the user edits the serial field post-activation

**File:** `src/activation_window.rs:491-493` (see also `activation_state.rs`)
**Issue:** `on_serial_changed()` is called each frame the serial input changes. It
clears `serial_error` and `show_rebind_confirm` but does **not** clear `agent_token`,
`tenant_id`, `pusher_key`, `pusher_cluster`, or `flow`. This creates a data-loss bug:

1. User enters serial A, presses "Ativar" → activation succeeds → `agent_token` is
   set, `flow = ValidatedAwaitingTestPrint`.
2. User edits the serial input (types serial B) → `on_serial_changed()` fires.
3. `state.agent_token.is_some()` is still `true` → button label reads "Salvar
   ativação", `can_save = true`.
4. User presses Enter or "Salvar ativação" → `handle_save()` saves **serial A's token**
   to DPAPI, with serial B visible in the input field.

The agent now authenticates to Noren using a token tied to serial A while the UI
appeared to show serial B was active.

**Fix:** `on_serial_changed()` must invalidate the token on any edit that happens after
a successful activation:
```rust
pub fn on_serial_changed(&mut self) {
    self.serial_error = None;
    self.show_rebind_confirm = false;
    // Invalidate prior activation result so a serial edit requires re-activation.
    if self.agent_token.is_some() {
        self.agent_token = None;
        self.tenant_id = None;
        self.pusher_key = None;
        self.pusher_cluster = None;
        self.enabled_types.clear();
        self.flow = FlowState::Idle;
        self.test_print_confirmed = None;
    }
}
```

---

### CR-05: `unsafe` env-var mutation in debug-mode tests is unsound under the multi-threaded test runner

**File:** `src/pusher/client.rs:443, 456, 465, 471`
**Issue:** Three tests call `unsafe { std::env::set_var(...) }` and
`unsafe { std::env::remove_var(...) }` with the comment "SAFETY: single-threaded test
process; no concurrent env access." This safety claim is false. Rust's default test
runner spawns multiple test threads in the same process. Concurrent calls to
`set_var`/`remove_var` and `var()` from different threads race on the underlying libc
`environ` structure, which is undefined behaviour on all major platforms.

Rust 1.81 made these functions `unsafe` precisely because of this hazard. The presence
of the `unsafe` block does not make the data race safe — it acknowledges the danger but
does not eliminate it.

A concrete failure: `fake_event_returns_false_when_env_not_set` calls `remove_var`
while `fake_event_parses_job_id_and_type` calls `set_var` concurrently; both access
the same key; one test could observe the other's env-var mutation and either fail or
(on some libc implementations) corrupt the env table.

**Fix (preferred):** Refactor `try_fake_pusher_event` to accept the raw value as a
parameter (or an `Option<&str>`) instead of reading `std::env::var` directly. Tests
call the function with explicit values, eliminating all env-var mutation:
```rust
#[cfg(debug_assertions)]
fn try_fake_pusher_event(tx: &mpsc::Sender<PrintEvent>, override_val: Option<&str>) -> bool {
    let raw = match override_val.or_else(|| std::env::var("BREVLY_FAKE_PUSHER_EVENT").ok().as_deref()) {
        Some(v) => v.to_string(),
        None => return false,
    };
    // ...
}
```

**Fix (minimal):** Add `#[serial_test::serial]` from the `serial_test` crate to
serialize all env-mutating tests, preventing concurrent access.

---

## Warnings

### WR-01: Empty Pusher credentials from ConfigStore produce unroutable URLs with no diagnostic

**File:** `src/main.rs:403-416`
**Issue:** All three `config_store::get` calls for `pusher_key`, `pusher_cluster`, and
`tenant_id` fall back to `unwrap_or_default()` (empty string `""`). If any credential
is missing (schema bug, partial activation, future migration issue), the Pusher loop
constructs `wss://ws-.pusher.com/app/?...` and `"private-tenant--print"`. The WS
connect fails with a DNS/TLS error on every attempt. The reconnect loop runs
indefinitely; the tray stays yellow; logs say "WS connect failed" with no indication
that the root cause is a missing config value.

**Fix:** Use `.filter(|s| !s.is_empty()).context(...)` to treat empty credentials as a
startup error:
```rust
let pusher_key = config_store::get(&conn, "pusher_key")
    .context("Failed to read pusher_key")?
    .filter(|s| !s.is_empty())
    .context("pusher_key is missing from ConfigStore — re-activate to restore")?;
```
Apply the same pattern to `pusher_cluster` and `tenant_id`.

---

### WR-02: Unreachable `Err(_) => String::new()` fallback silently produces an empty agent token

**File:** `src/main.rs:419-423`
**Issue:**
```rust
let agent_token = match &cred_result {
    Ok(bytes) => String::from_utf8(bytes.clone())
        .context("agentToken bytes are not valid UTF-8")?,
    Err(_) => String::new(), // unreachable on Runtime path (needs_activation=false)
};
```
The comment acknowledges unreachability. However, if a future refactor desynchronises
`is_runtime` and `cred_result` (e.g., `needs_activation` logic changes), this arm
silently produces an empty agent token. Every subsequent `pusher_auth()` POST will
receive `Authorization: Bearer ` (empty), which the backend will reject with 403. The
Pusher loop reconnects forever with no indication that the root cause is a missing
credential.

**Fix:** Replace the silent fallback with `unreachable!()`:
```rust
Err(e) => unreachable!(
    "Runtime path requires Ok credential, but got: {e}"
),
```

---

### WR-03: `pusher:error` event data is never logged — Pusher error codes are invisible to operators

**File:** `src/pusher/client.rs:321-323` and line 237-239
**Issue:** Both `pusher:error` handlers (the subscription-wait loop and the dispatch
loop) log a generic "reconnecting" message but do not decode or log `env.data`. Pusher
error events carry an error code and message: `{"code":4001,"message":"App key not
valid"}`. Error codes 4001–4009 cover invalid key, app disabled, over connection limit,
etc. Without logging this data, operators cannot distinguish a misconfigured key from a
capacity issue, both of which present identically as "yellow tray + reconnecting".

**Fix:** Extract and log the error payload in both error handlers:
```rust
"pusher:error" => {
    eprintln!(
        "[brevly-print] Pusher: received pusher:error — reconnecting. \
         Details: {}",
        env.data
    );
    break 'inner true;
}
```

---

### WR-04: `tokio::time::interval` uses `MissedTickBehavior::Burst` — spurious reconnect on system sleep/wake

**File:** `src/pusher/client.rs:270`
**Issue:** `interval(Duration::from_secs(30))` uses the tokio default
`MissedTickBehavior::Burst`. After a Windows machine sleeps and wakes, the timer
catches up by firing multiple ticks in rapid succession:

- Tick 1 fires → `awaiting_pong = false` (no pong yet) → ping is sent → `awaiting_pong = true`.
- Tick 2 fires milliseconds later → `awaiting_pong` is still `true` (no time for pong
  to arrive) → zombie detected → reconnect.

This causes a spurious reconnect after every sleep/wake, which is common on restaurant
PCs that idle overnight. `MissedTickBehavior::Delay` skips missed ticks and fires once,
which is the correct behaviour for a keepalive timer.

**Fix:**
```rust
use tokio::time::MissedTickBehavior;

let mut ping_timer = interval(Duration::from_secs(30));
ping_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
ping_timer.tick().await; // burn first immediate tick
```

---

### WR-05: `resp.error_for_status().unwrap_err()` panics if server returns an unexpected 2xx status

**File:** `src/noren_client.rs:136-140`
**Issue:**
```rust
_ => {
    Err(ActivateError::Transport(
        resp.error_for_status().unwrap_err(),
    ))
}
```
`reqwest::Response::error_for_status()` returns `Ok(response)` for 2xx status codes
and `Err(reqwest::Error)` for non-2xx. This catch-all branch handles all status codes
other than 200, 403, 404, 409. If the server returns 201 or 204 (legal 2xx codes that
some API gateways emit), `error_for_status()` returns `Ok(...)`, and `.unwrap_err()`
panics with "called `unwrap_err()` on an `Ok` value".

**Fix:**
```rust
status => {
    // Use status directly to avoid unwrap_err() on a potential 2xx
    let _ = resp; // drop body
    Err(ActivateError::Transport(
        reqwest::Error::from(
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("activate: unexpected HTTP status {status}"),
            )
        )
    ))
}
```
Or, more idiomatically, use `resp.error_for_status()` only after verifying the status
is non-2xx:
```rust
_ => match resp.error_for_status() {
    Ok(_) => Err(ActivateError::Transport(
        // Unreachable in practice but safe to model as a parse failure
        resp.json::<ActivateResponse>().await.unwrap_err()
    )),
    Err(e) => Err(ActivateError::Transport(e)),
},
```

---

## Info

### IN-01: Per-frame `.clone()` on `Option<String>` fields is unnecessary

**File:** `src/activation_window.rs:501, 758`
**Issue:**
```rust
if let Some(err) = &state.serial_error.clone() {   // line 501
if let Some(warn) = &state.autostart_warn.clone() { // line 758
```
Both clone an `Option<String>` to satisfy the borrow checker, but the clone is not
needed. `if let Some(ref err) = state.serial_error` (or `if let Some(err) =
&state.serial_error`) borrows the inner `&String` without allocating. The clone runs
every frame at 60+ fps.

**Fix:**
```rust
if let Some(err) = &state.serial_error {
if let Some(warn) = &state.autostart_warn {
```

---

### IN-02: `disconnected` binding in the inner loop is always `true` and is immediately suppressed

**File:** `src/pusher/client.rs:275, 360`
**Issue:**
```rust
let disconnected = 'inner: loop {
    // every break is `break 'inner true;`
};
let _ = disconnected; // logged above
```
Every code path that exits the inner loop uses `break 'inner true`. The variable is
always `true` and is immediately discarded with `let _ = ...`. The binding implies a
meaningful return value (e.g., graceful vs. error disconnection) that is not used.

**Fix:** Remove the binding; use a plain labeled loop:
```rust
'inner: loop {
    // ...
}
// After breaking, reconnect with backoff.
```

---

### IN-03: `attempt` counter uses plain `+= 1` without overflow guard (minor)

**File:** `src/pusher/client.rs:177, 192, 200, 211, 224, 260, 363`
**Issue:** `attempt` is `u32` and incremented with `attempt += 1` at six sites.
In debug builds this panics on overflow; in release builds it wraps to 0, briefly
resetting the backoff to 1 s. The practical risk is negligible (4.29 billion retries
at 60 s each ≈ 8 000 years), but since `backoff_delay` already clamps at
`attempt.min(6)`, nothing is gained from incrementing past 6. Saturating arithmetic
costs nothing and eliminates the theoretical hazard.

**Fix:** Replace all `attempt += 1` with `attempt = attempt.saturating_add(1);`.

---

_Reviewed: 2026-07-16T16:05:02Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
