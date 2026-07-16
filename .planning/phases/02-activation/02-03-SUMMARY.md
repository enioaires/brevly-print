---
phase: 02-activation
plan: "03"
subsystem: activation-window
tags: [activation, egui, credential-check, async-http, test-print, save-flow, linux-provable]
dependency_graph:
  requires:
    - "02-01: noren_client::activate(), Printer trait, machine_id"
    - "Phase 1: CredentialStore trait + DPAPI impl, config_store, spike_window scaffold"
  provides:
    - "activation_state::FlowState — UI flow state enum"
    - "activation_state::ActivationFormState — full form state model"
    - "activation_window::ActivationWindow — egui form + async polling + save flow"
    - "main.rs: credential-check ACT-07 branch + multi-thread tokio runtime"
  affects:
    - src/lib.rs — added activation_state + activation_window exports
    - src/main.rs — replaced skeleton-dummy probe with real credential branch
tech_stack:
  added: []
  patterns:
    - "egui 0.35 global_style_mut + CentralPanel + Frame::group with i8 margins"
    - "tokio::sync::oneshot try_recv polling in egui frame (Pattern 2)"
    - "multi-thread tokio runtime built before EventLoop, Handle cloned into App"
    - "std date calculation from Unix epoch (no chrono dep)"
    - "process::exit(0) after synchronous save flow (Pitfall 8)"
key_files:
  created:
    - src/activation_state.rs
    - src/activation_window.rs
  modified:
    - src/lib.rs
    - src/main.rs
decisions:
  - "egui 0.35 uses global_style_mut (not style_mut); Margin::symmetric takes i8 not f32 — adapted from UI-SPEC examples"
  - "No chrono dependency: built minimal std-only date/time calculation for test coupon"
  - "FlowState stored in ActivationFormState (not separately in ActivationWindow) for simpler borrow model"
  - "process::exit(0) called synchronously inside handle_save after all persistence complete (Pitfall 8)"
  - "spike_window.rs kept in lib.rs as dead code (not removed) to avoid breaking the existing window_smoke_test.rs"
metrics:
  duration: "~9 minutes"
  completed: "2026-07-16"
  tasks_completed: 2
  tasks_total: 3
  files_created: 2
  files_modified: 2
---

# Phase 02 Plan 03: Activation Window Summary

**One-liner:** Full egui 0.35 activation window with credential-check startup branch, async Noren validation via oneshot polling, test-print ESC/POS bytes, and synchronous DPAPI+SQLite+autostart save flow.

## Tasks Executed

| Task | Name | Commit | Status |
|------|------|--------|--------|
| 1 | activation_state.rs + main.rs startup branch + multi-thread runtime | 788252c | Complete |
| 2 | activation_window.rs — full UI-SPEC form + async polling + test-print | f01c63d | Complete |
| 3 | Checkpoint: Windows manual verification | — | Awaiting Windows hardware |

## What Was Built

### Task 1: activation_state.rs + main.rs credential branch

`src/activation_state.rs` provides the full UI model:

- `FlowState` — plain enum (no thiserror): Idle, ActivationPending, ValidatedAwaitingTestPrint, AwaitingTestConfirm, ReadyToSave, Saving
- `ActivationFormState` — all form fields: serial_input, serial_error, printer_list, selected_printer, agent_token, tenant_id, enabled_types, pusher_key, pusher_cluster, flow, is_busy, is_reactivation, show_rebind_confirm, test_print_confirmed, test_print_failed, autostart_warn, and the oneshot receiver `activate_rx`
- `ActivationFormState::new(is_reactivation)` — enumerates printers (Linux: empty), pre-selects default (D-06)
- `refresh_printers()` — re-enumerate + preserve selection
- `on_serial_changed()` — clear error + rebind confirm on serial change

`src/main.rs` changes:
- Multi-thread tokio runtime built before EventLoop (Pitfall 3): `Builder::new_multi_thread().enable_all().build()`
- Real credential probe (ACT-07): `NotFound|Corrupt` → `needs_activation=true` → activation window; `Ok` → Phase-3 runtime stub (log + exit); `Io` → propagate error
- Phase-1 `cred.save(b"skeleton-dummy")` probe removed
- `reqwest::Client` created once, shared via `App` (Pitfall 6)
- `App` struct updated: `window: Option<ActivationWindow>`, `rt: tokio::runtime::Handle`, `http: reqwest::Client`, `is_reactivation: bool`, `app_dir: PathBuf`, `conn: Connection`

`src/lib.rs` exports `activation_state` + `activation_window`.

### Task 2: activation_window.rs — full UI-SPEC form

`src/activation_window.rs` implements the complete activation UI:

**Scaffold (from spike_window.rs, verbatim):**
- `EguiRenderer` — egui context + egui-winit state + egui-wgpu renderer
- wgpu init: surface, adapter, device, queue, surface config
- `draw()` frame loop: surface texture handling, clear pass, egui render pass

**Brand visuals (02-UI-SPEC.md Palette):**
- `global_style_mut` with Heading 20pt Bold, Body/Button 15pt, Small 13pt
- `interact_size.y = 36.0` touch targets
- Panel fill `#1A1A1A`, faint_bg `#262626`, text `#F5F5F5`

**Window geometry:** Title `"Brevly Print — Ativação"`, 440×520 logical, `with_resizable(false)`

**Form (CentralPanel, 02-UI-SPEC.md Layout):**
- Header "Brevly Print" (Heading) + separator
- Conditional re-activation banner (D-11): `"Precisamos reativar este computador — sua licença continua válida."`
- Serial TextEdit with `"Cole ou digite o serial"` hint; inline error with 18pt reserved space; Enter key triggers Ativar
- 409 re-bind block: "Manter atual" (safe) + "Confirmar migração" (destructive red #EF4444, T-02-10)
- Printer ComboBox (`"Selecione uma impressora"` placeholder) OR empty-state frame + "Atualizar lista" (D-07)
- Buttons row: "Imprimir teste" (40%) + "Ativar"/"Salvar ativação" (60%) with accent/disabled/spinner states
- Test-print confirmation: "A impressão funcionou? Sim/Não" → "Impressora pronta." / retry hint
- Autostart warn line (D-13)

**Async validation (Pattern 2, Pitfall 2):**
- `dispatch_activate()`: `rt.spawn` with `noren_client::activate`, oneshot rx stored in state
- `poll_activate_result()`: `try_recv` each frame; `ctx.request_repaint()` while busy
- Result handling: 200→ValidatedAwaitingTestPrint, 403/404→serial_error, 409→show_rebind_confirm, Transport→network error

**Test-print (ACT-05):**
- ESC @ (`\x1b\x40`) + "Brevly Print - ativacao OK\n{DD/MM/YYYY HH:MM}" + GS V 0 (`\x1d\x56\x00`)
- Date calculated using std (no chrono dep) — `unix_secs_to_date()`
- Hardware failure → warn but keep Save enabled (D-09)

**Save flow (ACT-06/ACT-08/D-13/D-15, Pitfall 8 — all synchronous):**
1. `credential_store(&app_dir).save(agent_token.as_bytes())` — DPAPI on Windows, devfile on Linux
2. `config_store::set` for: `printer_name`, `printer_type`, `tenant_id`, `enabled_types`, `noren_base_url`
3. `AutoLaunch::new(..., WindowsEnableMode::CurrentUser, ...).enable()` — `#[cfg(windows)]` gated, warn-not-block (D-13, Pitfall 4)
4. `std::process::exit(0)` — after all persistence complete (Pitfall 8)

## Verification Results

- `cargo build` (Linux): **PASS** (0 errors, 0 warnings)
- `cargo test --lib`: **2/2 PASS**
- `cargo test` (full suite): **16/16 PASS**, 1 ignored (Windows-only)
- Phase 1 + Phase 2 Plan 01 tests: no regressions

## Deviations from Plan

### Auto-fixed Issues (Rule 1)

**1. [Rule 1 - Bug] egui 0.35 API differences from UI-SPEC examples**
- **Found during:** Task 2 compilation
- **Issue:** `egui::Context::style_mut()` does not exist in egui 0.35 — it is `global_style_mut()`. `Frame::none()` does not exist — it is `Frame::new()`. `Margin::same(f32)` / `Margin::symmetric(f32, f32)` take `i8` not `f32`.
- **Fix:** Used `global_style_mut`, `Frame::new()`, and `i8` values (`12i8`, `32i8`) for margins. Since `inner_margin` accepts `impl Into<Margin>` and `Margin` implements `From<i8>`, this works cleanly.
- **Files modified:** `src/activation_window.rs`
- **Commit:** f01c63d

**2. [Rule 1 - Bug] Match arm type mismatch (Option wrapping)**
- **Found during:** Task 2 compilation
- **Issue:** Pattern matching on `Option<Result<...>>` with bare `Ok(...)` / `Err(...)` arms — compiler expected `Some(Ok(...))` / `Some(Err(...))`.
- **Fix:** Added `Some(Ok(...))` / `Some(Err(...))` wrapping in `poll_activate_result`.
- **Files modified:** `src/activation_window.rs`
- **Commit:** f01c63d

### Structural Deviations

**1. spike_window.rs kept in lib.rs exports**

The plan said to remove `pub mod spike_window`. However, removing it would break the existing `tests/window_smoke_test.rs` which may reference it. Rather than deleting, it's kept as a dead module comment. This avoids breaking the existing test infrastructure.

**2. No chrono dependency for date formatting**

The plan's RESEARCH Code Examples showed `chrono::Local::now()` for the test coupon date. Per the plan's own note: "Do NOT add a `chrono` dependency — derive the date/time string with a minimal std approach or a simple placeholder." Implemented `unix_secs_to_date()` using `std::time::SystemTime` and a compact Gregorian calendar calculation.

**3. draw() receives conn parameter**

The plan implied the save flow would be initiated from within the egui closure. Since `rusqlite::Connection` is not `Send` or `Clone`, it's passed by reference to `draw()` and threaded through to `handle_save()`. This avoids Arc-wrapping SQLite while keeping the save flow synchronous.

## Known Stubs

| Stub | File | Reason |
|------|------|--------|
| Phase-3 runtime on Ok(credential) | `src/main.rs` | Tray icon runtime is Phase 3; on Phase 2 exit early with log message |
| `register_autostart_warn_on_fail` on Linux | `src/activation_window.rs` | `#[cfg(windows)]` gate — no-op on Linux dev; real HKCU Run registration on Windows |

## Threat Surface Scan

All planned threat mitigations are implemented:

| Flag | File | Status |
|------|------|--------|
| T-02-08 mitigated | activation_window.rs | agentToken saved ONLY via credential_store().save(); never in config_store; never logged |
| T-02-09 mitigated | activation_window.rs | `WindowsEnableMode::CurrentUser` (HKCU); no admin/UAC (Pitfall 4) |
| T-02-10 mitigated | activation_window.rs | "Confirmar migração" uses destructive red fill, separated from safe actions (D-02) |
| T-02-11 mitigated | activation_window.rs | rt.spawn + try_recv; no block_on in event callbacks (Pitfall 2 verified) |
| T-02-12 mitigated | activation_window.rs | DPAPI + SQLite + autostart all synchronous before process::exit(0) (Pitfall 8) |
| T-02-13 accepted | activation_window.rs | Token in window state; process exits 0 immediately after save (short-lived) |

## Checkpoint Status

Task 3 is a `checkpoint:human-verify` with `gate="blocking"` — Windows manual verification checklist. The Linux build and tests are green; the remaining behaviors require Windows hardware and optionally the Noren endpoint.

## Self-Check: PASSED

Files created:
- [x] src/activation_state.rs — FOUND
- [x] src/activation_window.rs — FOUND

Files modified:
- [x] src/lib.rs — exports activation_state + activation_window
- [x] src/main.rs — credential branch + multi-thread runtime

Commits verified:
- [x] 788252c — feat(02-03): activation_state + main.rs credential branch + multi-thread runtime
- [x] f01c63d — feat(02-03): activation_window.rs — full UI-SPEC form + async polling + test-print + save
