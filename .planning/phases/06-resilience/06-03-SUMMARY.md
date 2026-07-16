---
phase: 06-resilience
plan: "03"
subsystem: resilience/retry
tags: [retry, health-state, crash-recovery, toast, sqlite, tokio]
dependency_graph:
  requires: ["06-01", "06-02"]
  provides: ["run_retry_task", "retry_task module", "D-10 health strings"]
  affects: ["src/main.rs", "src/health_state.rs", "src/lib.rs"]
tech_stack:
  added: []
  patterns:
    - "Fourth WAL SQLite connection (D-04) with busy_timeout(5s)"
    - "Startup crash recovery query (D-05) for orphaned 'printing' rows"
    - "tokio::time::interval(5s) + MissedTickBehavior::Delay poll loop (D-06)"
    - "#[cfg(windows)] toast + #[cfg(not(windows))] stderr fallback (D-07)"
    - "AlwaysFailPrinter unit test mock for exhaustion invariant"
key_files:
  created:
    - src/retry_task.rs
  modified:
    - src/health_state.rs
    - src/lib.rs
    - src/main.rs
decisions:
  - "retry_base_url cloned from worker_base_url (not auth_url) because auth_url is moved into pusher_config before retry clones are taken"
  - "has_retry_printer guard on spawn so activation-incomplete state does not panic (consistent with print_worker behaviour)"
  - "crash recovery query_map error handled as Vec::new() (not return) so a prepare failure on one query does not kill the task"
metrics:
  duration: "~18 minutes"
  completed: "2026-07-16T19:33:43Z"
  tasks_completed: 3
  tasks_total: 3
  files_created: 1
  files_modified: 3
---

# Phase 06 Plan 03: Retry Task — Crash Recovery + Poll Loop + Exhaustion

Implements `run_retry_task` (fourth Tokio task) — crash recovery re-queuing orphaned `'printing'` rows at startup, 5-second poll loop retrying due `retry_queue` rows up to 3 times at 30-second intervals, exhaustion path setting `status='failed'` and sending `HealthState::Problem` + Windows toast, plus D-10 wording update on health strings.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Update health_state.rs Problem strings + declare retry_task module | d816314 | src/health_state.rs, src/lib.rs |
| 2 | Create src/retry_task.rs — crash recovery + poll loop + retry state machine | d393f1f | src/retry_task.rs |
| 3 | Spawn retry task in main.rs Runtime block | c1681fb | src/main.rs |

## Verification

- `cargo test retry_exhaustion` — PASS (1 passed, 1 ignored)
- `cargo test crash_recovery` — PASS (1 passed)
- `cargo test health_state` — PASS (2 passed)
- `cargo build` — PASS (0 crates compiled, all clean)
- Full suite `cargo test` — PASS (50 passed, 3 ignored)

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| `retry_base_url = worker_base_url.clone()` | `auth_url` is moved into `pusher_config` before the retry clone block; `worker_base_url` is already a clone of `auth_url` and is equivalent |
| `has_retry_printer` guard on retry spawn | Consistent with print_worker's hard-exit on missing printer; avoids spawning a task that would immediately return on WAL open |
| `query_map` error path returns `Vec::new()` (not `return`) | A prepare failure in the poll loop should log + skip this tick, not kill the entire retry task |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `unwrap_or_else` on `query_map` is type-incompatible on stable Rust**

- **Found during:** Task 2
- **Issue:** The PATTERNS.md snippet used `stmt.query_map(...).unwrap_or_else(|_| Box::new(std::iter::empty()))`, but `query_map` returns `MappedRows<'_, Closure>` which is not unifiable with `Box<Empty<_>>` in `unwrap_or_else`.
- **Fix:** Replaced with `match stmt.query_map(...) { Ok(rows) => rows.filter_map(...).collect(), Err(e) => { eprintln!(...); Vec::new() } }` in both the crash recovery startup block and the poll loop.
- **Files modified:** src/retry_task.rs
- **Commit:** d393f1f (included in the Task 2 commit)

## Known Stubs

None — all branches are fully implemented. The `#[cfg(not(windows))]` toast arm intentionally logs to stderr (documented cross-platform build requirement, not a stub).

## Threat Flags

No new threat surface beyond what the plan's threat model covers (T-06-04, T-06-05, T-06-06, T-06-SC all addressed):
- `agent_token` never appears in any `eprintln!` in `retry_task.rs` (T-06-04 mitigated)
- `busy_timeout(5s)` set on the WAL connection (T-06-06 mitigated)
- Toast call wrapped in `#[cfg(windows)]` (no untrusted input path)

## Self-Check: PASSED

- `src/retry_task.rs` exists: FOUND
- `src/health_state.rs` contains "Falha na impressora": FOUND (2 occurrences)
- `src/lib.rs` contains `pub mod retry_task`: FOUND
- `src/main.rs` contains `run_retry_task(`: FOUND
- Commit d816314 exists: FOUND
- Commit d393f1f exists: FOUND
- Commit c1681fb exists: FOUND
- `cargo test` 50 passed, 3 ignored: PASS
- `cargo build` clean: PASS
