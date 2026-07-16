---
phase: 03-tray-runtime-first-distributable
plan: 02
subsystem: tray-runtime
tags: [tray-icon, winit, windows, health-state, single-instance, autostart, velopack]
dependency_graph:
  requires: [03-01]
  provides: [tray-runtime, AppMode, UserEvent-health, single-instance-mutex, autostart-velopack-fix]
  affects: [main.rs, activation_window.rs]
tech_stack:
  added: []
  patterns:
    - "#![cfg(windows)] file-level gate on tray_runtime.rs (spooler.rs pattern)"
    - "AppMode enum for unified event-loop startup path"
    - "UserEvent::HealthChanged for cross-thread tray state transitions (D-04)"
    - "CreateMutexW Local\\ session-scoped single-instance guard (D-08)"
    - "TrayIconEvent::set_event_handler + MenuEvent::set_event_handler proxy pattern"
    - "Update.exe sibling detection for Velopack autostart path (RUN-03)"
key_files:
  created: []
  modified:
    - src/tray_runtime.rs
    - src/main.rs
    - src/activation_window.rs
decisions:
  - "UserEvent tray variants are #[cfg(windows)] inside the enum rather than a separate Windows-only enum — keeps event loop type portable while gating the platform-specific payload types"
  - "handle_menu_event is a cfg(windows) impl method on App — avoids a free function that would need App fields passed individually"
  - "Task 3 fix applied to activation_window.rs (not activation_state.rs) because that is where register_autostart_warn_on_fail actually lives"
metrics:
  duration: "~30 minutes"
  completed: "2026-07-16"
  tasks_completed: 3
  tasks_total: 3
  files_changed: 3
---

# Phase 03 Plan 02: Tray Runtime + AppMode Unification Summary

**One-liner:** Windows-only TrayRuntime with tri-color HealthState menu, AppMode-unified event loop, single-instance mutex, and Velopack-safe autostart path.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Create src/tray_runtime.rs — Windows-only tray icon, menu, MessageBoxW | abd2f72 | src/tray_runtime.rs |
| 2 | Restructure src/main.rs — AppMode, UserEvent, Runtime path, mutex, proxies | c5167c7 | src/main.rs |
| 3 | Fix src/activation_window.rs autostart path for Velopack (RUN-03) | e5d99c7 | src/activation_window.rs |

## What Was Built

### Task 1: `src/tray_runtime.rs`

Full Windows-only tray runtime module:

- `TrayRuntime::new(health: HealthState)` — creates tray with `with_menu_on_left_click(false)` (D-07), right-click menu with 4 items
- `apply_health(&self, health: HealthState)` — swaps icon, tooltip, and disabled status label atomically
- `build_tray_menu()` — status (disabled) + separator + Reativar + Sobre + separator + Sair (D-06)
- `show_about_dialog()` — `MessageBoxW` showing version + product name only (T-03-02-03: no tenant/serial/token)
- `confirm_quit_dialog()` — `MessageBoxW` with "Fechar o Brevly Print? As impressões vão parar enquanto o programa estiver fechado." (T-03-02-04)
- `to_wstr()` — WR-06 UTF-16 null-termination pattern (spooler.rs analog)
- `SAFETY:` comments on all unsafe blocks (project convention)
- `#[cfg(test)] menu_items_have_distinct_ids` test — portable Linux-runnable

### Task 2: `src/main.rs`

Surgical restructure:

- `enum AppMode { Activation, Runtime }` — no cfg gate, portable
- `UserEvent` enum extended: `TrayIconEvent` + `MenuEvent` variants are `#[cfg(windows)]`; `HealthChanged(HealthState)` is portable
- `App` struct extended: `mode`, `health`, `#[cfg(windows)] tray_runtime`, `activation_window` (renamed from `window`), `is_reactivation` preserved
- `new_events(Init)` — creates `TrayRuntime::new()` in Runtime mode (CRITICAL: after Win32 message pump is running)
- `resumed()` — branches on AppMode; Runtime branch is no-op
- `user_event()` — dispatches `#[cfg(windows)]` variants to `handle_menu_event()` or health swap
- `about_to_wait()` — Runtime branch does NOT call `request_redraw()` (ControlFlow::Wait is sufficient)
- `handle_menu_event()` — Reativar reopens activation window; Sobre calls `show_about_dialog()`; Sair calls `confirm_quit_dialog()` before exit
- Single-instance mutex: `CreateMutexW("Local\\BrevlyPrintAgent")` after Velopack bootstrapper (D-08/D-09)
- Event proxy wiring: `TrayIconEvent::set_event_handler` + `MenuEvent::set_event_handler` before `run_app()`
- Phase 3 early-exit stub **removed**; replaced with `AppMode`-based App construction

### Task 3: `src/activation_window.rs`

Autostart path fix in `register_autostart_warn_on_fail()`:

- Detects Velopack layout by checking if `parent.join("Update.exe").exists()`
- When detected: registers `grandparent/brevly-print.exe` (the stable Velopack stub)
- When not detected: registers `current_exe()` directly (dev builds, non-Velopack installs)
- Comment: `// RUN-03: Velopack installs to current\; autostart must point at root stub.`
- Inert on Linux (Update.exe will never exist on a dev Linux box)

## Verification Results

```
cargo build   → 0 errors, 1 warning (HealthChanged variant unused on Linux — expected)
cargo test    → 18 passed, 1 ignored (all prior Phase 1/2/3-01 tests green)
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] UserEvent tray variants needed #[cfg(windows)] inside the enum**
- **Found during:** Task 2 first build
- **Issue:** `tray_icon` is a `[target.'cfg(windows)'.dependencies]` crate. The plan's pattern shows the variants without cfg gates on the enum itself, but the enum lives in `main.rs` (which compiles on Linux). References to `tray_icon::TrayIconEvent` and `tray_icon::menu::MenuEvent` fail on Linux with "cannot find module or crate `tray_icon`".
- **Fix:** Added `#[cfg(windows)]` attribute to the two tray-specific variants inside the enum, and added matching `#[cfg(windows)]` to the match arms in `user_event()`.
- **Files modified:** `src/main.rs`
- **Commit:** c5167c7

**2. [Rule 3 - Blocking] Task 3 fix location is `activation_window.rs`, not `activation_state.rs`**
- **Found during:** Task 3 read-first phase
- **Issue:** The plan specifies `src/activation_state.rs` as the file containing the auto-launch registration. The actual `register_autostart_warn_on_fail()` function lives in `src/activation_window.rs`. `activation_state.rs` only contains the `ActivationFormState` struct fields (including `autostart_warn: Option<String>`) with no auto-launch logic.
- **Fix:** Applied the Velopack stub path detection to the correct file (`src/activation_window.rs`).
- **Files modified:** `src/activation_window.rs`
- **Commit:** e5d99c7

## Known Stubs

None — all plan goals achieved. The `HealthState::Connected` seed (D-02) is intentional: Phase 4 (Pusher) will drive real `HealthChanged` transitions. This is documented in the code.

## Threat Flags

No new security surface beyond what the threat model already covers:
- T-03-02-01: Named mutex DoS — accepted (Local\\ session-scoped)
- T-03-02-02: HKCU Run tampering — accepted (fundamental Windows design)
- T-03-02-03: About dialog info disclosure — mitigated (version only, no tenant/token)
- T-03-02-04: Unguarded Sair — mitigated (confirm_quit_dialog PT-BR message)
- T-03-02-05: Velopack autostart path mismatch — mitigated (Task 3 fix)

## Self-Check: PASSED

| Item | Status |
|------|--------|
| src/tray_runtime.rs | FOUND |
| src/main.rs | FOUND |
| src/activation_window.rs | FOUND |
| 03-02-SUMMARY.md | FOUND |
| abd2f72 (Task 1 commit) | FOUND |
| c5167c7 (Task 2 commit) | FOUND |
| e5d99c7 (Task 3 commit) | FOUND |
