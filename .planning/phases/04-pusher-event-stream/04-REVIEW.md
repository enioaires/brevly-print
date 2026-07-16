---
phase: 04-pusher-event-stream
reviewed: 2026-07-16T00:00:00Z
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
  critical: 2
  warning: 4
  info: 3
  total: 9
status: issues_found
---

# Phase 04: Code Review Report

**Reviewed:** 2026-07-16
**Depth:** standard
**Files Reviewed:** 9
**Status:** issues_found

## Summary

Reviewed the Phase 4 Pusher event stream implementation across the activation window, Noren client, Pusher reconnect loop, protocol parsers, backoff module, and integration tests. The overall architecture is sound: the dedup fence, backoff with jitter, zombie detection, and EVT-02 (fresh auth per reconnect) are correctly implemented. The protocol layer correctly handles double-decoding of Pusher data fields.

Two critical issues were found: a stale agent-token save path that lets a user save a credential from a prior serial activation after typing a new serial, and an environment variable race condition in debug-mode tests. Four warnings cover missing silent-failure handling for empty Pusher config, an unreachable branch that becomes a footgun, test stub fragility, and a potentially confusing Enter-key trigger. Three informational items cover dead crate dependencies, unnecessary per-frame clones, and a misleading variable name.

---

## Critical Issues

### CR-01: Stale agent-token can be saved after serial field edit

**File:** `src/activation_window.rs:491` (and `src/activation_state.rs:137`)

**Issue:** `on_serial_changed()` (called every frame the serial field changes) clears `serial_error` and `show_rebind_confirm` but does **not** clear `agent_token`, `tenant_id`, `pusher_key`, `pusher_cluster`, or `flow`. This means:

1. User enters serial A → presses "Ativar" → succeeds → `agent_token` is now set, `flow = ValidatedAwaitingTestPrint`.
2. User edits the serial field (typing serial B) → `on_serial_changed()` fires → `serial_error` cleared, `show_rebind_confirm` cleared.
3. At this point `state.agent_token.is_some()` is still `true`, so the primary button label becomes `"Salvar ativação"` and `can_save = true`.
4. User presses Enter or "Salvar ativação" → `handle_save()` executes → saves the **token from serial A** to DPAPI while the serial input field visually shows serial B.

The saved credential is silently wrong. The tray agent will then authenticate to Noren using a token tied to a serial the user believes they changed.

**Fix:** Add cleanup to `on_serial_changed()` whenever the serial changes after a successful activation:

```rust
// In src/activation_state.rs
pub fn on_serial_changed(&mut self) {
    self.serial_error = None;
    self.show_rebind_confirm = false;
    // If serial changes after a successful activation, invalidate the token
    // so the user must re-activate before saving.
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

### CR-02: Unsafe env-var mutation in tests is unsound in multi-threaded test runner

**File:** `src/pusher/client.rs:443-472`

**Issue:** Three debug-mode tests call `unsafe { std::env::set_var(...) }` and `unsafe { std::env::remove_var(...) }` with a comment that assumes single-threaded execution: "SAFETY: single-threaded test process; no concurrent env access." This assumption is incorrect. Rust's test harness runs tests in parallel by default (multiple threads in the same process), and `std::env::set_var`/`remove_var` are not thread-safe. Concurrent access to the process environment causes undefined behaviour (data race on the underlying libc `environ` pointer).

This is exactly why Rust 1.81 stabilized `std::env::set_var` as `unsafe` and emits a warning when called from multiple threads. In practice, another test running concurrently can read a polluted or partially-written env var, and the mutating test can race on remove_var.

**Fix:** Isolate env-var tests from the parallel runner. Two options:

Option A — use `serial_test` crate to serialize env-var tests:
```rust
#[serial_test::serial]
#[test]
fn fake_event_returns_false_on_malformed_value() { ... }
```

Option B — replace env-var injection with a parameterised function that accepts an optional override, and test that path directly without touching the process environment. This is the cleaner long-term fix and eliminates the `unsafe` blocks entirely.

---

## Warnings

### WR-01: Empty Pusher credentials in Runtime mode are silently accepted, producing an unroutable WebSocket URL

**File:** `src/main.rs:403-416`

**Issue:** When reading Pusher config from the ConfigStore in Runtime mode, all three `config_store::get` calls fall back to `unwrap_or_default()` (empty string `""`). If for any reason `pusher_key`, `pusher_cluster`, or `tenant_id` were never written to the store (e.g., a migration bug or a future schema change), the Pusher loop starts with `config.key = ""`, `config.cluster = ""`, and `config.tenant_id = ""`, producing:

- `ws_url = "wss://ws-.pusher.com/app/?protocol=7..."`
- `channel = "private-tenant--print"`

The WebSocket connect will fail (DNS resolution error), the backoff loop fires immediately at `attempt=0`, and the tray stays yellow forever — with no diagnostic in the logs about *why* (the error message says "WS connect failed" but not "Pusher key is empty").

**Fix:** Treat empty/missing Pusher credentials as a fatal startup error, not a silent fallback:

```rust
let pusher_key = config_store::get(&conn, "pusher_key")
    .context("Failed to read pusher_key")?
    .filter(|s| !s.is_empty())
    .context("pusher_key is missing from config — re-activate to restore")?;
```

---

### WR-02: Unreachable fallback arm for `agent_token` on the Runtime path creates a silent empty-string token

**File:** `src/main.rs:419-423`

**Issue:**

```rust
let agent_token = match &cred_result {
    Ok(bytes) => String::from_utf8(bytes.clone())
        .context("agentToken bytes are not valid UTF-8")?,
    Err(_) => String::new(), // unreachable on Runtime path (needs_activation=false)
};
```

The comment acknowledges this arm is unreachable. However, "unreachable in practice" branches that produce silent empty defaults are dangerous: if any future refactor changes how `needs_activation` is evaluated (or if `is_runtime` and `cred_result` get out of sync), the Pusher loop would start with `agent_token = ""`, producing 403 errors on every auth attempt with no clear error about the root cause.

**Fix:** Replace the unreachable arm with an explicit panic or an `unreachable!()` macro:

```rust
Err(e) => unreachable!(
    "Runtime path requires credential to be Ok, but got: {e}"
),
```

---

### WR-03: Test stub Content-Length is byte-counted on a `&str` — brittle for non-ASCII bodies

**File:** `tests/pusher_auth_test.rs:27`

**Issue:**

```rust
let response = format!(
    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n...",
    body.len()  // body is &'static str; .len() returns byte count, not char count
);
```

All current test bodies are ASCII, so `str::len()` (byte count) equals the character count. However, the function signature accepts `body: &'static str` with no ASCII constraint. If a future test passes a body containing multi-byte UTF-8 (e.g., a Portuguese error message), `body.len()` will return a byte count larger than the character count, the Content-Length header will be wrong, and `reqwest` may fail to parse the response with a confusing error that looks like a client bug.

**Fix:** Document the ASCII-only constraint or use `body.as_bytes().len()` (which is the same for ASCII but correctly named):

```rust
format!(
    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
    body.as_bytes().len()
)
```

---

### WR-04: Enter key in serial field triggers `handle_save()` when `agent_token` is already set, bypassing the visual "Salvar ativação" button intent

**File:** `src/activation_window.rs:669-698`

**Issue:** The `enter_in_serial` flag (line 496) fires whenever the serial field loses focus via Enter. When the button branch `can_save || can_activate` is active and `state.agent_token.is_some()`, pressing Enter sets `do_save = true` (line 676) — invoking `handle_save()` and closing the window. This means:

- A user who just activated and is reviewing the UI can inadvertently trigger the final save just by pressing Enter in the serial field (e.g., while trying to paste something).
- The Enter key bypasses the visual affordance of the "Salvar ativação" button, making the trigger non-obvious.

This is compounded by CR-01: if the user edited the serial, `agent_token` is still set, and Enter triggers a save of the stale token.

**Fix:** Restrict `enter_in_serial` to only trigger `dispatch_activate()`, not `handle_save()`. The save action should require an explicit button click:

```rust
// Enter in serial field only dispatches activation, never saves.
if ui.add(accent_button(primary_label)...).clicked() {
    if state.agent_token.is_some() { do_save = true; }
    else { do_activate = true; }
} else if enter_in_serial && state.agent_token.is_none() {
    do_activate = true;
}
```

---

## Info

### IN-01: `hmac` and `sha2` crates declared in Cargo.toml but never used in source

**File:** `Cargo.toml:45-46`

**Issue:** The Cargo.toml declares:
```toml
hmac = "0.13"
sha2 = "0.11"
```
with the comment "Pusher HMAC auth (Phase 4)". However, Pusher channel authentication is delegated entirely to the Noren backend (`noren_client::pusher_auth()` POSTs to `/api/agent/pusher/auth`). The agent itself never computes an HMAC; the server returns a pre-computed `auth` string. Neither `hmac` nor `sha2` is imported anywhere in the source tree. These are dead dependencies that inflate binary size and expand the supply-chain attack surface.

**Fix:** Remove both entries from `Cargo.toml`:
```toml
# Remove:
# hmac = "0.13"
# sha2 = "0.11"
```

---

### IN-02: Per-frame `.clone()` on `Option<String>` fields in the egui render closure

**File:** `src/activation_window.rs:501, 758`

**Issue:**

```rust
if let Some(err) = &state.serial_error.clone() {   // line 501
if let Some(warn) = &state.autostart_warn.clone() { // line 758
```

Both lines clone the `Option<String>` field to satisfy the borrow checker, but this is unnecessary. The pattern `if let Some(err) = &state.serial_error {` already borrows the inner `String` without allocating a clone. This clone runs every rendered frame (60+ fps) and allocates a heap `String` on each call even when the field is `None` (because `Option<String>::clone()` still inspects the discriminant).

**Fix:**

```rust
if let Some(err) = &state.serial_error {
if let Some(warn) = &state.autostart_warn {
```

---

### IN-03: `disconnected` boolean is always `true` and is immediately discarded

**File:** `src/pusher/client.rs:275, 360`

**Issue:**

```rust
let disconnected = 'inner: loop {
    // ... every break is `break 'inner true;`
};
let _ = disconnected; // logged above
```

Every `break` inside the inner loop evaluates to `break 'inner true;`. The variable can never be `false`. The `let _ = disconnected;` suppressor confirms it is unused after assignment. The variable name implies a meaningful return value that isn't there.

**Fix:** Remove the binding and use `'inner: loop` without capturing a return value. If a distinction between graceful vs. error disconnect is needed in the future, model it as an enum at that time:

```rust
'inner: loop {
    // ...
}
// After breaking, always reconnect with backoff.
```

---

_Reviewed: 2026-07-16_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
