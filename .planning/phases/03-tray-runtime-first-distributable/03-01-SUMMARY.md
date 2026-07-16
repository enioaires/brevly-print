---
phase: 03-tray-runtime-first-distributable
plan: "01"
subsystem: health-state
tags: [health-state, tray-icons, rgba-assets, cargo-features, windows-api]
dependency_graph:
  requires: []
  provides:
    - "HealthState enum (Connected/Reconnecting/Problem) with tooltip() and status_label()"
    - "cfg(windows) icon() accessor loading embedded RGBA assets"
    - "16x16 solid-color RGBA tray assets (green/yellow/red)"
    - "Win32_System_Threading and Win32_UI_WindowsAndMessaging features in Cargo.toml"
  affects:
    - "src/lib.rs (module declarations)"
    - "Cargo.toml (windows dep features extended)"
    - "03-02 (TrayRuntime uses HealthState::icon())"
tech_stack:
  added: []
  patterns:
    - "Portable enum with cfg(windows) extension block (same as activation_state.rs)"
    - "include_bytes! embedded RGBA assets for tray icons"
    - "Python3 one-liner for generating solid-color 16x16 RGBA binary files"
key_files:
  created:
    - src/health_state.rs
    - src/tray_runtime.rs
    - src/assets/tray_green.rgba
    - src/assets/tray_yellow.rgba
    - src/assets/tray_red.rgba
  modified:
    - src/lib.rs
    - Cargo.toml
decisions:
  - "Seeds Connected (green) on startup per D-02 — honest Phase 3 state"
  - "tray_runtime.rs is a placeholder comment; Plan 03-02 replaces with full impl"
  - "Assets committed as static files (simpler than build.rs); referenced via include_bytes!"
  - "Win32_System_Threading and Win32_UI_WindowsAndMessaging added to single windows dep entry"
metrics:
  duration_minutes: 5
  completed: "2026-07-16T14:14:46Z"
  tasks_completed: 2
  tasks_total: 2
  files_created: 5
  files_modified: 2
---

# Phase 3 Plan 01: HealthState Enum + Tray RGBA Assets Summary

**One-liner:** HealthState enum with PT-BR tooltip/status strings and cfg(windows) icon() accessor loading three embedded 16x16 solid-color RGBA tray assets (green/yellow/red).

## What Was Built

**Task 1 — src/health_state.rs + src/lib.rs**

Created `src/health_state.rs` with:
- `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum HealthState { Connected, Reconnecting, Problem }`
- `impl HealthState` with `tooltip()` returning PT-BR strings ("Brevly Print — Conectado" etc.) and `status_label()` returning short labels ("Conectado" etc.)
- `#[cfg(windows)] impl HealthState` with `icon()` loading 16x16 RGBA via `include_bytes!`
- `#[cfg(test)] mod tests` with two passing tests: `all_states_have_distinct_tooltips` and `status_labels_are_non_empty`

Created `src/tray_runtime.rs` as a single-comment placeholder (`// Phase 3, Plan 02`) so that the `pub mod tray_runtime` declaration in `lib.rs` compiles on Linux.

Updated `src/lib.rs` to add:
- `pub mod health_state;`
- `pub mod tray_runtime;`

**Task 2 — RGBA assets + Cargo.toml**

Generated three binary files using Python3:
- `src/assets/tray_green.rgba` — 256 pixels x [0x22, 0xC5, 0x5E, 0xFF] = 1024 bytes, color #22C55E
- `src/assets/tray_yellow.rgba` — 256 pixels x [0xF5, 0x9E, 0x0B, 0xFF] = 1024 bytes, color #F59E0B
- `src/assets/tray_red.rgba` — 256 pixels x [0xEF, 0x44, 0x44, 0xFF] = 1024 bytes, color #EF4444

Extended existing `[target.'cfg(windows)'.dependencies.windows]` entry in Cargo.toml with:
- `"Win32_System_Threading"` (for CreateMutexW in Plan 03-02)
- `"Win32_UI_WindowsAndMessaging"` (for MessageBoxW in Plan 03-02)

## Verification

- `cargo test health_state::tests` — 2/2 tests pass on Linux
- `cargo test` (full suite) — 18/18 tests pass on Linux (no regressions)
- `cargo build` (Linux) — exits 0, 0 crates compiled (incremental clean)
- `wc -c src/assets/*.rgba` — all three files exactly 1024 bytes
- Single `windows = { ... }` entry in Cargo.toml (no duplicates)

## Commits

| Task | Commit | Description |
|------|--------|-------------|
| Task 1 | 83979e4 | feat(03-01): add HealthState enum with tooltip/status_label and cfg(windows) icon accessor |
| Task 2 | 4b46289 | feat(03-01): generate 16x16 RGBA tray assets and extend windows crate features |

## Deviations from Plan

None — plan executed exactly as written.

The `src/tray_runtime.rs` placeholder was pre-announced in the acceptance criteria ("fix by creating an empty `src/tray_runtime.rs` placeholder"), so this is expected behavior, not a deviation.

## Threat Surface Scan

No new network endpoints, auth paths, file access patterns, or schema changes introduced. The `include_bytes!` embedded assets are checked at compile time (wrong dimensions would panic at runtime via Icon::from_rgba .expect()); the acceptance-criteria `wc -c` check enforces exactly 1024 bytes (addresses T-03-01-02). The tooltip strings contain only status text — no tenant ID, version number, or serial (addresses T-03-01-01).

## Self-Check: PASSED

- [x] `src/health_state.rs` exists and contains `pub enum HealthState`
- [x] `src/tray_runtime.rs` exists (placeholder)
- [x] `src/assets/tray_green.rgba` exists, 1024 bytes
- [x] `src/assets/tray_yellow.rgba` exists, 1024 bytes
- [x] `src/assets/tray_red.rgba` exists, 1024 bytes
- [x] `src/lib.rs` contains `pub mod health_state` and `pub mod tray_runtime`
- [x] `Cargo.toml` has `Win32_System_Threading` and `Win32_UI_WindowsAndMessaging`
- [x] Commits 83979e4 and 4b46289 exist in git log
- [x] 18 tests pass, 0 failures
