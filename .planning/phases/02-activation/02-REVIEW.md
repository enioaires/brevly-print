---
phase: 02-activation
reviewed: 2026-07-15T00:00:00Z
depth: standard
files_reviewed: 14
files_reviewed_list:
  - Cargo.toml
  - src/activation_state.rs
  - src/activation_window.rs
  - src/lib.rs
  - src/machine_id.rs
  - src/main.rs
  - src/noren_client.rs
  - src/printer/error.rs
  - src/printer/mod.rs
  - src/printer/serial.rs
  - src/printer/spooler.rs
  - src/printer/stub.rs
  - tests/noren_client_test.rs
  - tests/printer_test.rs
findings:
  critical: 5
  warning: 6
  info: 4
  total: 15
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-07-15T00:00:00Z
**Depth:** standard
**Files Reviewed:** 14
**Status:** issues_found

## Summary

Phase 02 implements the activation window (egui/winit/wgpu), the Noren HTTP client, the printer abstraction (spooler + serial + Linux stub), machine-ID reader, and the full save-flow (DPAPI → SQLite → autostart). The architecture is generally sound — the cfg-gated trait pattern is applied consistently, the DPAPI path is correctly walled behind `#[cfg(windows)]`, and the load-bearing C1 pitfall (`pDatatype = "RAW"`) is correctly addressed in the spooler.

However, five critical defects were found that would cause data loss, crashes, or broken behavior in production:

1. `std::process::exit(0)` is called inside the save flow while the SQLite `Connection` and tokio `Runtime` are still alive — this bypasses all `Drop` destructors and risks database corruption.
2. The credential check in `main.rs` calls `cred.load()` a second time to set `is_reactivation`, racing after the first call already confirmed `NotFound`.
3. The `dispatch_activate` re-bind path silently sends a new activation without any re-bind force flag — `AlreadyActiveOther` will loop forever.
4. The `WritePrinter` call truncates `data.len()` to `u32` silently on large payloads without a bounds check.
5. The serial port is flushed nowhere — ESC/POS bytes may remain in the OS buffer and never reach the printer.

---

## Structural Findings (fallow)

No structural pre-pass provided.

---

## Narrative Findings (AI reviewer)

## Critical Issues

### CR-01: `std::process::exit(0)` bypasses SQLite `Drop`, risking database corruption

**File:** `src/activation_window.rs:989`

**Issue:** `handle_save()` calls `std::process::exit(0)` while `conn: &rusqlite::Connection` is still live. `process::exit` does not run Rust destructors. The `rusqlite::Connection` destructor normally calls `sqlite3_close`, which flushes the WAL and releases file locks. Skipping this can leave `state.db` in a corrupt or half-written WAL state — precisely the state we want to avoid after a successful activation save. The `should_exit` flag on line 988 is set just before the `exit` call, making it dead code.

**Fix:** Remove `std::process::exit(0)`. Let `should_exit = true` propagate back to `main.rs` via the existing `window.should_exit()` check (line 100 of `main.rs`), which already calls `event_loop.exit()`. The event loop exits cleanly, destructors run, and the process terminates normally.

```rust
// In handle_save() — remove the process::exit call entirely:
*should_exit = true;
// std::process::exit(0);  <-- DELETE THIS LINE
```

---

### CR-02: Double `cred.load()` call in `main.rs` — TOCTOU and logic error

**File:** `src/main.rs:213`

**Issue:** `cred.load()` is called once at line 175 to determine `needs_activation`, and then called a **second time** at line 213 to set `is_reactivation`:

```rust
is_reactivation: matches!(cred.load(), Err(CredentialError::Corrupt(_))),
```

This is a Time-Of-Check-Time-Of-Use defect. Between the two calls, the credential file state can change (e.g., another process writes it, or a partial file appears). More concretely: if the first call returned `Err(CredentialError::NotFound)`, the code correctly opens the activation window — but `is_reactivation` will be `false` even though the file is missing. If the second call now returns `Corrupt` (because a corrupt file was created between the two calls), `is_reactivation` would be `true` for a `NotFound` scenario. The intent is to set `is_reactivation = true` only for `Corrupt`, but this logic is fragile and incorrect under any change to the underlying file.

**Fix:** Capture the first result and pattern-match it in both places:

```rust
let cred_result = cred.load();
let needs_activation = matches!(
    &cred_result,
    Err(CredentialError::NotFound) | Err(CredentialError::Corrupt(_))
);
// ... (Phase 3 stub exit) ...
let mut app = App {
    // ...
    is_reactivation: matches!(cred_result, Err(CredentialError::Corrupt(_))),
    // ...
};
```

---

### CR-03: Re-bind confirm dispatches `activate()` without a force-rebind flag — infinite 409 loop

**File:** `src/activation_window.rs:546`

**Issue:** When the server returns 409 (`AlreadyActiveOther`), `show_rebind_confirm` is set to `true` and the user sees the "Confirmar migração" dialog. Clicking it calls `dispatch_activate(state, rt, http)` — the exact same call that produced the 409. The `activate()` function in `noren_client.rs` sends `{ serial, machineId? }` with no additional flag to signal that the user has confirmed the re-bind. The server has no way to distinguish a re-bind confirmation from the original request and will return 409 again, producing an infinite loop: click "Confirmar migração" → another 409 → `show_rebind_confirm = true` again.

**Fix:** The Noren API must accept a `force_rebind: true` field (or similar) to authorise the migration. `dispatch_activate` needs a `force_rebind: bool` parameter:

```rust
// noren_client.rs — extend the request type
#[derive(Serialize)]
struct ActivateRequest<'a> {
    serial: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    machine_id: Option<&'a str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    force_rebind: bool,
}

// activation_window.rs — pass the flag
fn dispatch_activate(state, rt, http, force_rebind: bool) { ... }
// caller at rebind confirm:
dispatch_activate(state, rt, http, true);
// normal activate:
dispatch_activate(state, rt, http, false);
```

If the backend does not yet support this flag, at minimum the UI must not call `dispatch_activate` again after a 409 — doing so today silently loops.

---

### CR-04: `data.len() as u32` truncates silently for large ESC/POS payloads

**File:** `src/printer/spooler.rs:115`

**Issue:**

```rust
let write_ok: BOOL = WritePrinter(
    handle,
    data.as_ptr() as *const _,
    data.len() as u32,   // <-- silent truncation if data.len() > u32::MAX
    &mut bytes_written,
);
```

On a 64-bit process `usize` is 8 bytes; `u32` is 4 bytes. If `data.len()` exceeds `u32::MAX` (≈4 GiB), the cast silently wraps, and `WritePrinter` writes a different (shorter) byte count than intended with no error reported. While a single ESC/POS job is unlikely to exceed 4 GiB today, the correct pattern is an explicit guard:

**Fix:**

```rust
let len_u32 = u32::try_from(data.len()).map_err(|_| {
    PrinterError::PrintFailed(format!(
        "Print data too large for WritePrinter ({} bytes > u32::MAX)",
        data.len()
    ))
})?;

let write_ok: BOOL = WritePrinter(handle, data.as_ptr() as *const _, len_u32, &mut bytes_written);
```

---

### CR-05: Serial port not flushed — ESC/POS bytes may stay in OS buffer

**File:** `src/printer/serial.rs:35-36`

**Issue:**

```rust
port.write_all(bytes).map_err(PrinterError::Io)?;
Ok(())
```

`serialport` implements `std::io::Write`, but `write_all` only guarantees the bytes are written to the OS serial buffer — it does not flush the hardware FIFO or signal the driver to push bytes onto the wire. Without an explicit `port.flush()`, the last chunk of ESC/POS data (including the paper-cut command `\x1d\x56\x00`) may remain in the OS buffer if the port closes before the driver drains it. This would produce a test coupon that prints partial content (no cut) — a silent failure.

**Fix:**

```rust
port.write_all(bytes).map_err(PrinterError::Io)?;
port.flush().map_err(PrinterError::Io)?;
Ok(())
```

---

## Warnings

### WR-01: `EndPagePrinter` / `EndDocPrinter` not called on `WritePrinter` error before closing handle

**File:** `src/printer/spooler.rs:119-126`

**Issue:** When `WritePrinter` fails, the code calls `EndPagePrinter` and `EndDocPrinter` in a best-effort pattern before returning the error. However, if `StartPagePrinter` succeeded but `WritePrinter` failed, `EndPagePrinter` must be called before `EndDocPrinter` — the current code does both unconditionally even if `StartPagePrinter` previously failed. More critically, the outer `write_raw_to_spooler` will then call `ClosePrinter` on a handle where `EndDocPrinter` may not have been called (e.g., if `StartDocPrinterW` succeeds but something else fails before the `WritePrinter` block). The doc-started-but-not-ended state can leave a zombie job in the spooler queue.

**Fix:** Track whether `StartDocPrinterW` and `StartPagePrinter` have been called and call the matching `End*` functions on all error paths:

```rust
// After StartDocPrinterW:
let job_started = true;
// After StartPagePrinter:
let page_started = true;
// On any subsequent error:
if page_started { let _ = EndPagePrinter(handle); }
if job_started  { let _ = EndDocPrinter(handle); }
return Err(...);
```

---

### WR-02: `egui::CentralPanel::default().show(ui, ...)` called inside an existing `ui` — wrong API usage

**File:** `src/activation_window.rs:369`

**Issue:** `EguiRenderer::draw` receives a `run_ui: impl FnMut(&mut egui::Ui)` closure that is called inside `self.context.run_ui(raw_input, run_ui)`. In egui 0.35, `run_ui` passes a `&mut egui::Ui` scoped to the root viewport. Calling `egui::CentralPanel::default().show(ui, ...)` on an already-scoped `ui` argument — rather than on `ctx` — may work at runtime but is semantically wrong: `CentralPanel` should be added to the `egui::Context`, not to a child `Ui`. The correct pattern in egui 0.35 is:

```rust
self.context.run(raw_input, |ctx| {
    egui::CentralPanel::default().show(ctx, |ui| { ... });
});
```

Using `ui.show` instead of `ctx.show` means the panel does not properly fill the available viewport, and scroll areas or window decorations may misbehave.

---

### WR-03: `state.serial_input.clone()` every frame — unnecessary allocation in hot path

**File:** `src/activation_window.rs:485`

**Issue:**

```rust
let serial_before = state.serial_input.clone();
// ...
if state.serial_input != serial_before {
```

This clones the serial string every egui frame (60+ fps), even when the user is not typing. The correct approach for change detection in egui is to use the `TextEdit` response `.changed()` flag:

```rust
let serial_response = ui.add(egui::TextEdit::singleline(&mut state.serial_input)...);
if serial_response.changed() {
    state.on_serial_changed();
}
```

This avoids the allocation entirely.

---

### WR-04: `state.printer_list.clone()` inside the ComboBox `show_ui` closure

**File:** `src/activation_window.rs:591`

**Issue:**

```rust
for printer in state.printer_list.clone() {
```

`state.printer_list` is cloned in its entirety each frame so that the closure can iterate while `state.selected_printer` is mutably borrowed by `selectable_value`. The correct fix is to collect only the display names before the closure — `PrinterEntry` is `Clone` but contains two `String` fields, making per-frame cloning of the entire list wasteful:

```rust
let printer_names: Vec<String> = state.printer_list.iter()
    .map(|p| p.display_name.clone())
    .collect();
egui::ComboBox::from_label("")
    // ...
    .show_ui(ui, |ui| {
        for name in &printer_names {
            ui.selectable_value(&mut state.selected_printer, Some(name.clone()), name);
        }
    });
```

---

### WR-05: `noren_client_test` stub does not fully drain request before responding — can cause `reqwest` connection reset

**File:** `tests/noren_client_test.rs:36`

**Issue:**

```rust
let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
```

The stub reads at most 4096 bytes of the request, then immediately writes the response and closes the socket. If the `reqwest` POST body (serialized JSON + headers) exceeds 4096 bytes, `reqwest` will still be writing when the stub closes the socket, causing `reqwest` to see a `ConnectionReset` or broken-pipe error — and the test will fail non-deterministically. HTTP/1.1 responses are valid before the request is fully sent (e.g., early 4xx), but closing the write half first is safer:

```rust
// Read until headers end (look for \r\n\r\n) rather than reading a fixed chunk.
// Or at minimum, use shutdown(Write) on the socket before closing.
socket.shutdown().await.ok();
```

---

### WR-06: `doc_name` and `datatype` in `submit_job` have embedded null — `encode_utf16` on a string already ending in `\0` double-null-terminates

**File:** `src/printer/spooler.rs:81-85`

**Issue:**

```rust
let doc_name: Vec<u16> = "BrevlyPrint\0".encode_utf16().collect();
let datatype: Vec<u16>  = "RAW\0".encode_utf16().collect();
```

`encode_utf16()` encodes the `\0` character as the UTF-16 code unit `0x0000`. The resulting `Vec<u16>` is `[B, r, e, v, l, y, P, r, i, n, t, 0x0000]` — this is correct. However, this is also fragile: if the pattern was written expecting `chain(std::iter::once(0))` (which is used on line 57 for the printer name), the fact that a different termination style is used here is inconsistent and error-prone. The real bug is that `name_w` on line 55–58 uses `.chain(std::iter::once(0))` (correct null-terminator), while `doc_name` and `datatype` embed the `\0` inside the string literal. If a future maintainer applies the same literal-null pattern to a string that already has its own `\0` before the literal one, or copies the `chain` pattern to `doc_name`/`datatype`, the null terminator will be doubled. Standardise on one approach.

**Fix:** Use the same `.chain(std::iter::once(0))` pattern for all Win32 strings:

```rust
let doc_name: Vec<u16> = "BrevlyPrint".encode_utf16().chain(std::iter::once(0)).collect();
let datatype: Vec<u16> = "RAW".encode_utf16().chain(std::iter::once(0)).collect();
```

---

## Info

### IN-01: `println!` used for operational logging — no structured log or level filtering

**File:** `src/activation_window.rs:247`, `src/main.rs:162`, `src/main.rs:168`, `src/main.rs:179`, `src/main.rs:185`, `src/main.rs:196`

**Issue:** Multiple `println!` and `eprintln!` calls are used as the logging mechanism. For a tray agent that runs silently, stdout/stderr go nowhere visible to the user or to a log file. When a production bug occurs, these messages will be unrecoverable. The project has `anyhow` but no `tracing` or `log` crate.

**Fix:** Add `tracing` (or `log` + `env_logger`) to the dependency list and route all diagnostics through it. This also enables file-based logging for field debugging without a debugger attached.

---

### IN-02: `unwrap_or_default()` on `duration_since(UNIX_EPOCH)` silently returns 0 on clock skew

**File:** `src/activation_window.rs:849-851`

**Issue:**

```rust
let secs = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs();
```

`duration_since` returns `Err` if system time is before the epoch (misconfigured clocks or VM time drift). `unwrap_or_default()` silently returns 0, producing `01/01/1970 00:00` on the test coupon. While cosmetically harmless, this silent fallback can confuse field technicians who see a 1970 timestamp and assume the coupon is stale or from a different machine.

**Fix:** At minimum, log the time error; consider using `SystemTime::now().elapsed().unwrap_or_default()` differently, or just accept the 1970 fallback but format it as "N/A" when the epoch offset would be zero.

---

### IN-03: `Cargo.toml` references `hmac = "0.13"` and `sha2 = "0.11"` — these versions do not exist on crates.io as of current release

**File:** `Cargo.toml:44-45`

**Issue:**

```toml
hmac = "0.13"
sha2 = "0.11"
```

The latest stable RustCrypto releases as of this review are `hmac = "0.12"` and `sha2 = "0.10"`. Versions `0.13` and `0.11` do not exist and will fail `cargo build` with "no matching package". These are listed as Phase 4 dependencies, so build failure may be discovered late.

**Fix:** Correct to the current published versions:

```toml
hmac = "0.12"
sha2 = "0.10"
```

Verify against crates.io before Phase 4 begins.

---

### IN-04: `velopack = "1"` in `Cargo.toml` Windows-only section — but `velopack` crate's Rust SDK publishes under `0.0.x`, not `1.x`

**File:** `Cargo.toml:82`

**Issue:** The CLAUDE.md technology table states `velopack` Rust crate version `0.0.x (mirrors velopack core 1.x)`. The `Cargo.toml` specifies `velopack = "1"`, which requests a `^1.0.0` SemVer range. If the crate is published at `0.0.x`, this will fail to resolve. Conversely, if the crate has since published a 1.0, the API may differ from what was researched.

**Fix:** Verify the exact published version on crates.io and pin accordingly:

```toml
velopack = "0.1"   # or whatever is current — verify before Phase 7
```

---

_Reviewed: 2026-07-15T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
