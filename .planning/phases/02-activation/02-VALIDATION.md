---
phase: 2
slug: activation
status: draft
nyquist_compliant: false
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

> Populated by the planner / nyquist auditor from the plan tasks. Reference the
> "## Validation Architecture" section of 02-RESEARCH.md for what is Linux-testable
> vs Windows-only-verifiable.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| _pending planner population_ | | | | | | | | | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] _pending planner population_

*If none: "Existing infrastructure covers all phase requirements."*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Test-print bytes reach real thermal printer | ACT-05 | Requires physical USB/serial thermal printer hardware | On Windows box: select printer, click "Imprimir teste", confirm a legible coupon prints + paper cuts |
| Real DPAPI round-trip of agentToken | ACT-06 | DPAPI Scope::User only real on Windows | On Windows: activate, then relaunch — token decrypts, no re-activation |
| Autostart registers in HKCU Run | ACT-08 | Windows registry | On Windows: after save, check `HKCU\...\Run` has entry; reboot → agent launches |
| Re-activation on DPAPI key loss | ACT-07 | Requires simulated Windows reinstall / key loss | On Windows: corrupt/delete credential blob → relaunch → activation window reopens blank |

*If none: "All phase behaviors have automated verification."*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
