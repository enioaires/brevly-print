---
status: partial
phase: 07-auto-update-distribution-polish
source: [07-VERIFICATION.md, 07-UAT.md]
started: 2026-07-16T18:00:00Z
updated: 2026-07-16T18:00:00Z
---

## Current Test

[awaiting human testing on a Windows machine]

## Tests

### 1. UAT-07-01 — OQ-1 spike: staged `.nupkg` persistence past the 60s updater timeout
expected: On a Windows machine with a Velopack-installed BrevlyPrint v0.1.0, publish a local v0.1.1 feed (`releases.win.json` + `.nupkg`) reachable by `HttpSource`, let the update loop call `stage_update` (`download_updates` + `wait_exit_then_apply_updates(&update.to_apply, true, false, [])`), keep the process alive >90s, then inspect `%LocalAppData%\BrevlyPrint\packages\`. If the `.nupkg` persists → Design A confirmed (apply.rs correct as-is). If lost → switch to Design B (split stage/apply; `apply_staged_on_exit()` wired to "Sair"). Also confirm `update.to_apply` is the correct `UpdateInfo` field on velopack 1.2.0 (else fix apply.rs line ~92).
result: [pending]

### 2. SC-1 — Background download does not interrupt printing + one toast fires
expected: A print job completes in <1s while an update downloads; tray icon color unchanged; status line shows "Atualização pronta — será aplicada ao reiniciar"; exactly one toast fires on the first poll and not again on the 6h re-poll. Requires Windows + thermal printer + reachable `/api/agent/version`.
result: [pending]

### 3. UAT-07-02 / SC-2 on Windows — SHA256 mismatch aborts cleanly on the real apply path
expected: With a tampered artifact (server `sha256` ≠ hosted `.nupkg`), no tray change, no toast; agent still running as v0.1.0 after relaunch. (Linux proves `Ok(false)` via `sc2_mismatch_aborts_without_staging`, but `apply.rs` is `#![cfg(windows)]` and excluded from the Linux build.)
result: [pending]

### 4. SC-3 — New version runs after next reboot with no owner action
expected: After staging completes (tray shows "Atualização pronta"), reboot/relaunch; `VelopackApp::build().run()` (already first call in main.rs) applies the staged update; the Sobre dialog reports v0.1.1 — no owner action.
result: [pending]

## Summary

total: 4
passed: 0
issues: 0
pending: 4
skipped: 0
blocked: 0

## Gaps

None — these are hardware-gated verifications, not implementation gaps. All Linux-provable
components are implemented and green (29 lib tests + integration suites). To close: run these
4 tests on a Windows machine after a `cargo build --target x86_64-pc-windows-msvc` and a
first Velopack release, then `/gsd:verify-work 7`.
