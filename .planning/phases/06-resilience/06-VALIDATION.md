---
phase: 6
slug: resilience
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-07-16
---

# Phase 6 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test harness (`cargo test`) |
| **Config file** | none — `cargo test` discovers tests automatically |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 06-W0-01 | W0 | 0 | RES-01 | — | N/A | unit stub | `cargo test retry_task` | ❌ Wave 0 | ⬜ pending |
| 06-W0-02 | W0 | 0 | RES-01 | — | N/A | unit stub | `cargo test retry_queue_insert` | ❌ Wave 0 | ⬜ pending |
| 06-W0-03 | W0 | 0 | RES-02 | — | N/A | unit stub | `cargo test retry_exhaustion` | ❌ Wave 0 | ⬜ pending |
| 06-W0-04 | W0 | 0 | RES-03 | T-input-validation | validate_job_id() called on each job_id from pending pull | unit stub | `cargo test fetch_pending_jobs` | ❌ Wave 0 | ⬜ pending |
| 06-W0-05 | W0 | 0 | RES-04 | — | N/A | unit stub | `cargo test crash_recovery` | ❌ Wave 0 | ⬜ pending |
| 06-migration | W1 | 1 | RES-04 | — | N/A | unit | `cargo test migration` | ✅ (update v1→v2) | ⬜ pending |
| 06-fence | W2 | 2 | RES-01/04 | — | N/A | unit | `cargo test retry_queue_insert` | ❌ Wave 0 | ⬜ pending |
| 06-retry-task | W2 | 2 | RES-01/02/04 | — | N/A | unit | `cargo test retry_task` | ❌ Wave 0 | ⬜ pending |
| 06-exhaustion | W2 | 2 | RES-02 | — | N/A | unit | `cargo test retry_exhaustion` | ❌ Wave 0 | ⬜ pending |
| 06-crash-recovery | W2 | 2 | RES-04 | — | N/A | unit | `cargo test crash_recovery` | ❌ Wave 0 | ⬜ pending |
| 06-pending-pull | W3 | 3 | RES-03 | CR-02 path traversal | validate_job_id() on each job_id from fetch_pending_jobs | unit (HTTP stub) | `cargo test fetch_pending_jobs` | ❌ Wave 0 | ⬜ pending |
| 06-health-strings | W3 | 3 | RES-02 | — | N/A | unit | `cargo test health_state` | ✅ (update strings) | ⬜ pending |
| 06-dedup | W3 | 3 | RES-03 | — | N/A | unit | existing `insert_print_job_returns_false_on_duplicate` | ✅ | ⬜ pending |
| 06-main-spawn | W4 | 4 | RES-01/02/03/04 | — | N/A | build | `cargo build` | — | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/retry_task_test.rs` — stubs for RES-01 (retry queue logic), RES-02 (exhaustion path, send_health(Problem) call), RES-04 (crash recovery query: status='printing' rows not in retry_queue)
- [ ] `tests/noren_client_test.rs` or `tests/pending_jobs_test.rs` — stubs for RES-03 (fetch_pending_jobs 200, error path, validate_job_id called)
- [ ] Update `tests/config_store_test.rs` — change `user_version` assertion from 1 → 2 after migration v2 runs

*Existing dedup test (`insert_print_job_returns_false_on_duplicate`) covers RES-03 dedup path — no new stub needed.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Windows toast appears after 3 retries | RES-02 | `tauri-winrt-notification` is `#[cfg(windows)]`; CI runs Linux | On Windows dev machine: configure mock printer that always fails, trigger a print job, confirm toast appears after ~90s (3 × 30s) |
| Tray icon turns red after retry exhaustion | RES-02 | Tray icon requires Windows WM; CI runs Linux | Same session as toast test — confirm tray icon color change |
| Print job recovered after power-cycle crash | RES-04 | Requires process kill mid-print | Manually: start agent, kill process during print, restart, confirm job prints |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
