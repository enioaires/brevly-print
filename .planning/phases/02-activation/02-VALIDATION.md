---
phase: 2
slug: activation
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-07-15
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `cargo test` (integration tests under `tests/`) |
| **Config file** | none — Cargo built-in |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test` (Linux gate) + `cargo test` on windows-msvc CI (DPAPI/printing) |
| **Estimated runtime** | ~10 seconds (Linux, debug) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test` (full)
- **Before `/gsd:verify-work`:** Full suite must be green on Linux; Windows-only paths confirmed on windows CI / owner box
- **Max feedback latency:** ~30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-01-T1 | 01 | 1 | ACT-03 | T-02-01, T-02-02 | TLS-validated activate; token not logged | unit (mock HTTP) | `cargo test --test noren_client_test` | ❌ (Wave 0 creates) | ⬜ pending |
| 02-01-T2 | 01 | 1 | ACT-04, ACT-05 | T-02-03 | cfg-gated stub; no hardware on Linux | unit (stub) | `cargo test --test printer_test` | ❌ (Wave 0 creates) | ⬜ pending |
| 02-01-T3 | 01 | 1 | ACT-04, ACT-05 | T-02-SC | package legitimacy before build | manual checkpoint | (human-verify, blocking) | n/a | ⬜ pending |
| 02-02-T1 | 02 | 2 | ACT-05 | T-02-05, T-02-06 | RAW datatype (C1); handle closed on all paths | source-assert + windows-msvc build | `grep RAW src/printer/spooler.rs`; `cargo build --target x86_64-pc-windows-msvc` (CI) | ⬜ | ⬜ pending |
| 02-02-T2 | 02 | 2 | ACT-04 | T-02-05 | correct "(USB)"/"(Serial)" labels | source-assert + windows-msvc build | `cargo build --target x86_64-pc-windows-msvc` (CI); `cargo test` (Linux, empty path) | ⬜ | ⬜ pending |
| 02-03-T1 | 03 | 2 | ACT-07 | T-02-11 | NotFound/Corrupt → activation; multi-thread rt | unit (lib) | `cargo test --lib` | ⬜ | ⬜ pending |
| 02-03-T2 | 03 | 2 | ACT-02, ACT-05, ACT-06, ACT-08 | T-02-08..T-02-13 | async no-freeze; DPAPI token; save-before-exit; HKCU | unit (lib) + source-assert | `cargo test --lib`; grep coupon bytes + process::exit | ⬜ | ⬜ pending |
| 02-03-T3 | 03 | 2 | ACT-05, ACT-06, ACT-07, ACT-08 | T-02-05, T-02-08, T-02-09 | Windows E2E (RAW print, DPAPI round-trip, HKCU, re-activation) | manual checkpoint | Windows Manual Checklist | n/a | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Wave-0 test scaffolds + contract files are created inside Plan 01 (they ARE the plan's deliverables,
not a separate pre-step):

- [ ] `tests/noren_client_test.rs` — ACT-03 status/transport mapping (mock HTTP; Linux-runnable, no live Noren)
- [ ] `tests/printer_test.rs` — ACT-04/ACT-05 Linux stub path (empty list + Ok print)
- [ ] `src/noren_client.rs` — ActivateRequest/ActivateResponse/ActivateError + activate()
- [ ] `src/printer/{mod,error,stub}.rs` — Printer trait + PrinterEntry + StubPrinter
- [ ] `src/machine_id.rs` — MachineGuid reader (cfg-gated)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Test-print bytes reach real thermal printer (RAW datatype works) | ACT-05 | Requires physical USB/serial thermal printer hardware | On Windows box: select printer, click "Imprimir teste", confirm a legible coupon prints + paper cuts |
| Real DPAPI round-trip of agentToken | ACT-06 | DPAPI Scope::User only real on Windows | On Windows: activate, then relaunch — token decrypts, no re-activation |
| Autostart registers in HKCU Run | ACT-08 | Windows registry | On Windows: after save, check `HKCU\...\Run` has entry; reboot → agent launches |
| Re-activation on DPAPI key loss | ACT-07 | Requires simulated Windows reinstall / key loss | On Windows: corrupt/delete credential blob → relaunch → activation window reopens blank + reassuring banner |
| End-to-end serial validation | ACT-03 | Blocked on live Noren `POST /api/agent/activate` | With endpoint live: valid serial advances, invalid shows inline error |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies (Windows-only tasks use source-assert + windows-msvc CI build; hardware behaviors are manual checkpoints)
- [x] Sampling continuity: no 3 consecutive tasks without automated verify (each auto task runs `cargo test`/`cargo build`)
- [x] Wave 0 covers all MISSING references (Plan 01 creates the test files + contract modules)
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** planner-populated 2026-07-15
