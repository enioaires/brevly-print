---
phase: 7
slug: auto-update-distribution-polish
status: approved
nyquist_compliant: true
wave_0_complete: true
created: 2026-07-16
---

# Phase 7 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`cargo test`) — existing project tests use this |
| **Config file** | `Cargo.toml` `[dev-dependencies]` + `tests/` directory |
| **Quick run command** | `cargo test --lib` (Linux-safe, no Windows deps) |
| **Full suite command** | `cargo test` (Linux; Windows: `cargo test --target x86_64-pc-windows-msvc`) |
| **Estimated runtime** | ~15 seconds (Linux full suite) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd:verify-work`:** Full suite green on Linux + Windows smoke (manual install + version-bump apply test)
- **Max feedback latency:** ~15 seconds (Linux)

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 7-01-01 | 01 | 0 | DIST-03 | T-7-V6 | `verify_sha256(correct_bytes, correct_hex)` → `Ok(())` | unit (Linux) | `cargo test --lib update::verify` | ❌ W0 | ⬜ pending |
| 7-01-02 | 01 | 0 | DIST-03 | T-7-tamper | `verify_sha256(tampered_bytes, hex)` → `Err` | unit (Linux) | `cargo test --lib update::verify` | ❌ W0 | ⬜ pending |
| 7-01-03 | 01 | 0 | DIST-03 | T-7-V5 | `verify_sha256(&[], hex)` / malformed hex → `Err`; UPPER-case hex matches | unit (Linux) | `cargo test --lib update::verify` | ❌ W0 | ⬜ pending |
| 7-01-04 | 01 | 0 | DIST-02 | T-7-V5 | `check_for_update("0.1.0","0.2.0")` → `UpdateAvailable`; reverse → `UpToDate`; `"invalid"` → `Err` | unit (Linux) | `cargo test --lib update::check` | ❌ W0 | ⬜ pending |
| 7-01-05 | 01 | 0 | DIST-02/03 | T-7-mismatch | SC-2: on SHA256 mismatch, no `UpdateStaged` sent + Windows `stage_update()` never invoked | unit (Linux, mocked apply) | `cargo test --lib update` | ❌ W0 | ⬜ pending |
| 7-01-06 | 01 | 0 | DIST-02 | T-7-DoS | update task does not panic on HTTP error (mock HTTP failure) | unit (Linux, mock HTTP) | `cargo test --lib update` | ❌ W0 | ⬜ pending |
| 7-02-xx | 02 | 1 | DIST-02 | T-7-tamper | end-to-end stage → apply-on-next-boot on a real Velopack-installed binary | integration (Windows, manual) | manual (Velopack-installed binary) | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/update/mod.rs` — module skeleton (RED)
- [ ] `src/update/check.rs` — `check_for_update` + `UpdateDecision` enum + unit tests (semver compare edge cases)
- [ ] `src/update/verify.rs` — `verify_sha256(bytes, expected_hex) -> Result<()>` + unit tests (match / mismatch / case-insensitive / malformed / empty)
- [ ] `src/update/apply.rs` — `#[cfg(windows)]` real `stage_update()` + `#[cfg(not(windows))]` no-op stub (Linux-provable seam)
- [ ] `tests/update_task_test.rs` (or `#[cfg(test)]` in-module) — `try_check_and_stage` with mock HTTP; SC-2 abort assertion

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Staged `.nupkg` applies on next launch, agent runs new version | DIST-02 (SC-3) | Rust SDK has no `TestVelopackLocator`; `UpdateManager::new()` errors in dev builds — requires a real Velopack-installed binary | Install v0.1.0 via `Setup.exe`, publish a v0.1.1 feed + endpoint, let the agent stage it, reboot, confirm `Sobre` dialog shows v0.1.1 with no manual action |
| Background download does not interrupt an in-flight print | DIST-02 (SC-1) | Timing/hardware behavior on real printer | Trigger a print while a (throttled) update download runs; confirm comanda prints < 1s |
| SC-2 abort on real tampered artifact | DIST-03 (SC-2) | Confirms the Windows apply path is truly not reached | Serve a `sha256` that doesn't match the hosted `.nupkg`; confirm next launch is still the old version, no tray change |
| **SPIKE (OQ-1):** `.nupkg` persists on disk after `wait_exit_then_apply_updates` 60s timeout on a long-running process | DIST-02 | Determines whether stage-from-background-task is viable or apply must be deferred to process exit | On a Velopack-installed binary, call the stage path from a long-running process; verify pending update survives to next launch. **Wave 0 blocking spike.** |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies (verified by plan-checker Dimension 8a — all auto tasks have `cargo test`/`cargo build`/static-check commands)
- [x] Sampling continuity: no 3 consecutive tasks without automated verify (Dimension 8c pass)
- [x] Wave 0 covers all MISSING references (Wave 0 files delivered by Plan 01; no `<automated>MISSING</automated>` refs)
- [x] No watch-mode flags
- [x] Feedback latency < 15s (all quick `cargo test --lib` / static checks)
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-07-16 (plan-checker Dimension 8 pass; structure verified pre-execution)
