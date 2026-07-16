---
phase: 3
slug: tray-runtime-first-distributable
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-07-16
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `#[test]` + `cargo test` |
| **Config file** | `Cargo.toml` `[profile.test]` |
| **Quick run command** | `cargo test` (Linux — portable logic tests) |
| **Full suite command** | `cargo test --target x86_64-pc-windows-msvc` (Windows CI) |
| **Estimated runtime** | ~15 seconds (Linux quick), ~90 seconds (Windows CI) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test --target x86_64-pc-windows-msvc` in CI
- **Before `/gsd:verify-work`:** Full suite must be green + manual checklist complete
- **Max feedback latency:** 15 seconds (Linux), 90 seconds (Windows CI)

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 03-01-01 | 01 | 1 | RUN-02 | — | HealthState enum variants correct | unit | `cargo test health_state::tests` | ❌ W0 | ⬜ pending |
| 03-01-02 | 01 | 1 | RUN-02 | — | All three HealthState transitions reachable | unit | `cargo test health_state::tests::transitions` | ❌ W0 | ⬜ pending |
| 03-01-03 | 01 | 1 | RUN-01 | — | Menu action dispatch logic (item ID → action enum) | unit | `cargo test tray_runtime::tests::menu_dispatch` | ❌ W0 | ⬜ pending |
| 03-01-04 | 01 | 1 | RUN-01 | — | AppMode transitions (Activation → Runtime → Reactivation) | unit | `cargo test app_mode::tests` | ❌ W0 | ⬜ pending |
| 03-02-01 | 02 | 1 | RUN-01 | D-08 | Single-instance guard result handling | unit (mock) | `cargo test single_instance::tests` | ❌ W0 | ⬜ pending |
| 03-03-01 | 03 | 2 | DIST-01 | — | vpk pack produces Setup.exe | CI artifact | Windows CI job | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/tray/health_state.rs` — `HealthState` enum with unit tests
- [ ] `src/tray/runtime.rs` — `menu_dispatch` unit tests
- [ ] `src/app_mode.rs` — `AppMode` transitions unit tests
- [ ] `src/single_instance.rs` — result handling unit tests (mock Mutex)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Reboot → tray icon appears, no user action | RUN-03, SC-1 | Requires Windows hardware + reboot | Install dev build, reboot, observe tray in system tray area |
| Tri-color tray state (green/yellow/red) | RUN-02, SC-2 | Windows-only visual rendering | Trigger each `HealthState` via test command, observe icon color |
| No visible window in runtime mode | RUN-01, SC-3 | OS-level window state | Observe no taskbar entry; Task Manager shows process but no window |
| Signed installer — no SmartScreen block | DIST-01, SC-4 | Requires OV cert + Windows install | Install OV-signed `Setup.exe`, observe no SmartScreen warning |
| Right-click menu appears, all items work | RUN-01, D-06 | Windows GUI interaction | Right-click tray icon, verify status line / Reativar / Sobre / Sair |
| Single-instance guard — second launch exits | RUN-01, D-08 | Process lifecycle | Launch two instances, second exits silently without dialog |

---

## Phase Requirements → Test Coverage

| Req ID | Automated | Manual | Gate |
|--------|-----------|--------|------|
| RUN-01 | menu dispatch, AppMode transitions | No window/taskbar entry; right-click menu; single-instance | Both |
| RUN-02 | HealthState enum + transitions | Tray icon color changes | Both |
| RUN-03 | — | Reboot → autostart → tray appears | Manual only |
| DIST-01 | CI artifact check (Setup.exe) + signtool step | No SmartScreen block | Both |
