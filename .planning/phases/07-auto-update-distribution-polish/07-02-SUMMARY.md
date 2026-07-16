---
phase: 07-auto-update-distribution-polish
plan: 02
subsystem: update
tags: [update, velopack, tray, toast, event-loop, background-task, uat, linux-provable]
requirements: [DIST-02, DIST-03]

dependency_graph:
  requires:
    - 07-01 (verify_sha256, check_for_update, try_check_and_stage, apply.rs skeleton)
  provides:
    - src/update/apply.rs::stage_update (finalized Design A + OQ-1 comment)
    - src/main.rs::UserEvent::UpdateStaged
    - src/main.rs::run_update_check_loop
    - src/main.rs::show_update_ready_toast
    - src/tray_runtime.rs::TrayRuntime::set_update_status
    - .planning/phases/07-auto-update-distribution-polish/07-UAT.md
  affects:
    - src/main.rs (UserEvent enum + user_event arm + two new fns + fifth spawn)
    - src/tray_runtime.rs (set_update_status method)
    - src/update/apply.rs (SPIKE comments replaced; OQ-1/OQ-2 notes added)

tech_stack:
  added: []
  patterns:
    - UserEvent extension pattern: append variant, add arm in user_event(), mirror HealthChanged shape
    - Background task spawn pattern: clone before move, rt_handle.spawn(async move {...})
    - show_update_ready_toast: identical to show_print_failure_toast (retry_task.rs:488-504)
    - set_update_status: mirrors apply_health but omits set_icon (D-04)
    - apply.rs Design A: immediate wait_exit_then_apply_updates per RESEARCH.md default

key_files:
  created:
    - .planning/phases/07-auto-update-distribution-polish/07-UAT.md
  modified:
    - src/update/apply.rs
    - src/main.rs
    - src/tray_runtime.rs

decisions:
  - OQ-1 resolved to Design A (immediate call) per RESEARCH.md recommendation; UNVERIFIED on
    Windows — UAT-07-01 records the confirmation steps
  - update_token/update_base_url cloned from retry_token/retry_base_url BEFORE retry spawn move
    (agent_token is already moved into pusher spawn and unavailable)
  - UpdateStaged variant has no cfg gate (portable) — only the tray mutation inside the arm is
    cfg(windows)-gated, consistent with HealthChanged pattern
  - show_update_ready_toast placed in main.rs (same file as run_update_check_loop) to keep the
    UserEvent → toast wiring self-contained in the binary crate

metrics:
  duration: "~15m"
  completed: "2026-07-16"
  tasks_completed: 2
  tasks_uat_pending: 2
  files_changed: 4
---

# Phase 07 Plan 02: Windows Apply Half + Event Wiring

Finalized Velopack `stage_update` (Design A), wired `UserEvent::UpdateStaged` + `run_update_check_loop` as the fifth Tokio sibling, added `TrayRuntime::set_update_status` and `show_update_ready_toast` — the tray status line and one-shot toast when a staged update is ready.

## Environment Note

This plan was executed in a **Linux dev session** (no `x86_64-pc-windows-msvc` target available).

- `src/update/apply.rs` is `#![cfg(windows)]` — it does NOT compile on Linux. It is written correctly per RESEARCH.md Pattern 1 but **cannot be verified until a Windows build** (`cargo build --target x86_64-pc-windows-msvc`).
- `src/main.rs` and `src/tray_runtime.rs` Windows-gated code paths (tray mutation, `#[cfg(windows)]` in `user_event()`, `tauri-winrt-notification` Toast) are correct per established patterns but also require a Windows compile to catch any compile-time issues not reachable from Linux.
- Everything verifiable on Linux (binary build, lib tests, integration tests, acceptance source assertions) passed.

## What Was Built

### Task 1: OQ-1 Design Decision (auto-resolved per RESEARCH.md default)

OQ-1 (staging persistence past the 60s updater timeout) cannot be spike-validated in a Linux session. Design was resolved to **Design A** (immediate `wait_exit_then_apply_updates` call) per the RESEARCH.md recommendation:

> "RESEARCH.md recommends Design A as the default; the staged `.nupkg` is written to the packages directory before the updater process is spawned, and the bootstrapper (`VelopackApp::build().run()`) is what performs the swap on next launch."

**UAT-07-01** recorded in `07-UAT.md` with exact Windows confirmation steps (what directory to inspect, what constitutes Design A vs. Design B, and how to switch to Design B if needed).

### Task 2: Finalized `apply.rs` (Design A)

`src/update/apply.rs` — SPIKE comment placeholders removed; replaced with:

- Module-level doc comment explaining Design A vs. Design B with full rationale and exact steps to switch if UAT-07-01 returns Design B.
- `stage_update()` function doc with OQ-1 and OQ-2 (field name) uncertainty explicitly noted.
- Inline comment at the `wait_exit_then_apply_updates` call: `// OQ-1: staging-persistence past the 60s updater timeout is UNVERIFIED on Linux — confirm on Windows (see 07-02 UAT UAT-07-01).`
- `update.to_apply` field used (ASSUMED — RESEARCH.md A2). Inline comment flags this for Windows compile confirmation.

All four Velopack SDK calls (`UpdateManager::new`, `check_for_updates`, `download_updates`, `wait_exit_then_apply_updates`) are `?`-propagated via `.map_err(|e| anyhow::anyhow!("step: {e}"))` — no `.unwrap()` or `.expect()` on SDK results.

### Task 3: Wiring (`main.rs` + `tray_runtime.rs`)

**`src/main.rs`:**

1. `UserEvent::UpdateStaged` variant added (portable, no `cfg` gate — mirrors `HealthChanged`).
2. `user_event()` arm for `UpdateStaged`: calls `rt.set_update_status()` (`#[cfg(windows)]`), then `show_update_ready_toast()` (cfg-gated inside), then `let _ = event_loop` to suppress unused-variable warning on Linux.
3. `show_update_ready_toast()` free function — `tauri_winrt_notification::Toast` on Windows, `eprintln!` stub on Linux. Identical pattern to `show_print_failure_toast` in `retry_task.rs:488-504`.
4. `run_update_check_loop()` async function — 10s startup delay (D-03), `update_staged` bool gate (once-per-session, D-04 anti-double-toast), `brevly_print::update::try_check_and_stage` call, 6-hour poll sleep.
5. Fifth Tokio spawn inside `if is_runtime {` block — `update_token` and `update_base_url` cloned from `retry_token`/`retry_base_url` BEFORE the retry spawn move, so no borrow-after-move issue.
6. `VelopackApp::build().run()` remains the first `#[cfg(windows)]` call in `main()` (OQ3/D-09 — unchanged).

**`src/tray_runtime.rs`:**

`set_update_status(&self)` added to `impl TrayRuntime`:
- `self.menu_items.status.set_text("Atualização pronta — será aplicada ao reiniciar")`
- `self.tray.set_tooltip(Some("Brevly Print — Atualização pronta"))`
- **No `set_icon` call** — tray icon color is reserved for connection health (D-04).

### Task 4: Windows E2E (recorded as UAT)

Cannot be executed in a Linux dev session. **UAT-07-02** recorded in `07-UAT.md` with SC-1 / SC-2 / SC-3 exact test steps:
- SC-1: no print interruption, no icon color change, one toast, correct status line
- SC-2: SHA256 mismatch aborts without any tray change or version bump
- SC-3: new version runs after reboot with zero manual action

## Verified on Linux

| Assertion | Result |
|-----------|--------|
| `cargo build` (Linux host binary) | PASS |
| `cargo test --lib` (29 tests) | PASS |
| `cargo test` (69 tests, 1 ignored) | PASS |
| `UserEvent::UpdateStaged` in enum | FOUND (main.rs:73) |
| `UserEvent::UpdateStaged =>` arm in `user_event()` | FOUND (main.rs) |
| `update_token = retry_token.clone()` (not agent_token) | FOUND (main.rs:590) |
| `pub fn set_update_status` in tray_runtime | FOUND |
| `set_update_status` body has no `set_icon` | CONFIRMED |
| `VelopackApp::build().run()` still first `#[cfg(windows)]` call | CONFIRMED (line 357) |
| `apply.rs` contains all 4 Velopack SDK calls | CONFIRMED (4 calls, 16 hits) |
| No `.unwrap()`/`.expect()` on SDK results in apply.rs | CONFIRMED |
| `agent_token` never in `eprintln!`/`format!` in update path | CONFIRMED (T-02-02) |

## UNVERIFIED (Needs Windows Build)

1. `src/update/apply.rs` — `#![cfg(windows)]` file excluded from Linux compilation. Must be built with `cargo build --target x86_64-pc-windows-msvc` to verify:
   - `update.to_apply` field name is correct on `velopack::UpdateInfo` (OQ-2)
   - `std::iter::empty::<&str>()` satisfies the args parameter type
   - No unexpected type errors in the Velopack 1.2.0 API call chain

2. `src/main.rs` + `src/tray_runtime.rs` Windows-gated paths — `tauri_winrt_notification::Toast`, `TrayIconEvent::set_event_handler`, `MenuEvent::set_event_handler`, `set_update_status()` tray API calls require Windows target to compile fully.

## Deviations from Plan

### Auto-resolved Design Decision (Task 1)

**Deviation type:** OQ-1 blocking checkpoint replaced with design decision per RESEARCH.md default.

- **Found during:** Task 1 — `checkpoint:human-verify` cannot run in Linux session.
- **Resolution:** Applied Design A (immediate `wait_exit_then_apply_updates`) per RESEARCH.md §Pattern 7 recommendation and execution instruction: "implement the apply path as `wait_exit_then_apply_updates(&asset, /*silent*/ true, /*restart*/ false, vec![])` ... This is the documented Velopack pattern."
- **Files modified:** `src/update/apply.rs`, `.planning/phases/07-auto-update-distribution-polish/07-UAT.md`
- **Commits:** 026dab8

### Clone Order Fix (Rule 1 - Bug)

**Deviation type:** Structural correction to preserve Rust ownership invariants.

- **Found during:** Task 3 spawn wiring.
- **Issue:** The plan said to clone `retry_token`/`retry_base_url` AFTER the retry spawn, but the retry spawn moves those values into the closure when `printer_for_retry.is_some()`. Accessing them after the if-let block would be a compile-time ownership error.
- **Fix:** Moved the `update_token`/`update_base_url`/`update_http` clones to BEFORE the retry `if let Some(...)` block, so they capture their values before the retry spawn potentially moves the originals.
- **Files modified:** `src/main.rs`
- **Commit:** ef4a6b3

## Known Stubs

- `src/update/apply.rs` — entire file is `#![cfg(windows)]` and UNVERIFIED on Linux. The function body is correct per RESEARCH.md but has two unresolved assumptions (OQ-1 design, OQ-2 field name) that require Windows compilation and UAT-07-01 to confirm. Not a user-visible stub.

## Threat Surface Scan

All security-relevant surface introduced in this plan is covered by the plan's `<threat_model>`:

| Threat | Mitigation | Status |
|--------|-----------|--------|
| T-7-tamper: apply path only reached after verify_sha256 | apply.rs only called from try_check_and_stage after verify passes | IMPLEMENTED |
| T-7-C2: tray mutation off-thread | UpdateStaged flows through EventLoopProxy → event-loop thread → set_update_status | IMPLEMENTED |
| T-7-2xtoast: double toast on re-poll | update_staged bool flag in run_update_check_loop gates toast to once per session | IMPLEMENTED |
| T-7-V2: agent_token disclosure | update_token only via .bearer_auth() in check_version; eprintln! only logs {e:#} | IMPLEMENTED |

No new trust boundary surface beyond what the threat model covers.

## Self-Check: PASSED

Files verified to exist:
- src/update/apply.rs: FOUND
- src/main.rs (UserEvent::UpdateStaged): FOUND
- src/main.rs (run_update_check_loop): FOUND
- src/tray_runtime.rs (set_update_status): FOUND
- .planning/phases/07-auto-update-distribution-polish/07-UAT.md: FOUND

Commits verified:
- 026dab8: FOUND (apply.rs finalized + UAT file)
- ef4a6b3: FOUND (main.rs + tray_runtime.rs wiring)

Tests: 29/29 lib + 69/69 full suite green on Linux.

## Commits

| Task | Commit | Description |
|------|--------|-------------|
| 1+2  | 026dab8 | feat(07-02): finalize apply.rs Design A + record UAT-07-01/UAT-07-02 |
| 3    | ef4a6b3 | feat(07-02): wire UpdateStaged event, fifth update task, tray status + toast |
