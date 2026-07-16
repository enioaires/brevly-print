---
phase: 5
slug: job-pipeline
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-07-16
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`#[test]`, `#[tokio::test]`) |
| **Config file** | none (Cargo default) |
| **Quick run command** | `cargo test --lib -q` |
| **Full suite command** | `cargo test -q` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib -q`
- **After every plan wave:** Run `cargo test -q`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 5-01-01 | 01 | 1 | PRT-01 | — | base64 decode from `{"bytes":"<b64>"}` | unit | `cargo test -q fetch_job_bytes` | ❌ Wave 0 | ⬜ pending |
| 5-01-02 | 01 | 1 | PRT-01 | — | non-200 status returns Err | unit | `cargo test -q fetch_job_bytes` | ❌ Wave 0 | ⬜ pending |
| 5-01-03 | 01 | 1 | PRT-08 | — | 409 ack response is Ok(()) | unit | `cargo test -q ack_job` | ❌ Wave 0 | ⬜ pending |
| 5-01-04 | 01 | 1 | PRT-09 | T-02-02 | bearer_auth never logs token | unit | `cargo test -q fetch_job_bytes` | ❌ Wave 0 | ⬜ pending |
| 5-02-01 | 02 | 1 | PRT-09 | — | disabled job_type: UPDATE to 'printed' + ack, no print | unit | `cargo test -q enabled_types_filter` | ❌ Wave 0 | ⬜ pending |
| 5-02-02 | 02 | 1 | PRT-08 | — | SQLite UPDATE precedes ack_job call (ordering) | integration | `cargo test -q print_worker_test` | ❌ Wave 0 | ⬜ pending |
| 5-02-03 | 02 | 1 | PRT-02/03/04 | — | all job types routed through same print path | unit | `cargo test -q print_worker_test` | ❌ Wave 0 | ⬜ pending |
| 5-02-04 | 02 | 1 | PRT-05 | — | StubPrinter returns Ok(()) on Linux for Spooler and Serial | unit | `cargo test printer_test` | ✅ `tests/printer_test.rs` | ⬜ pending |
| 5-02-05 | 02 | 1 | PRT-07 | — | Phase 4 dedup fence: second same job_id not delivered | unit | `cargo test -q insert_print_job` | ✅ `src/pusher/client.rs` | ⬜ pending |
| 5-03-01 | 03 | 2 | PRT-06 | — | < 1 second latency from event to print | manual | Windows integration test | ❌ manual-only | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/print_worker_test.rs` — stubs for PRT-01 (`fetch_job_bytes` base64 decode, non-200 Err), PRT-08 (`ack_job` 409 → Ok(()), C4 ordering), PRT-09 (enabled_types filter), PRT-02/03/04 (all job types same path)
- [ ] `src/print_worker.rs` (skeleton) — module must exist for unit tests to import

*Existing infrastructure covers: PRT-05 (`tests/printer_test.rs`), PRT-07 (Phase 4 tests in `src/pusher/client.rs`).*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Print latency < 1 second from Pusher event arrival to physical print | PRT-06 | Requires physical thermal printer on Windows; no hardware on Linux CI | On Windows with thermal printer: observe timestamp of Pusher event in logs vs. time of physical print; must be < 1 second |
| USB spooler path (WritePrinter RAW) prints ESC/POS correctly | PRT-05 | Hardware-dependent; StubPrinter on CI always succeeds | Connect USB thermal printer; fire test print event; verify formatted receipt prints (not ASCII garbage) |
| COM port path (serialport) prints ESC/POS correctly | PRT-05 | Hardware-dependent | Connect COM port thermal printer; fire test print event; verify formatted receipt |
| Job type strings match Noren Pusher emit values | D-06/PRT-09 | Requires grep of Noren source repo | `grep -r "job_type\|'order'\|'dispatch'\|'closing'" ~/repos/brevly/noren` and verify values match enabled_types filter strings |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
