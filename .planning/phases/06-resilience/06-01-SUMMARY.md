---
phase: 06-resilience
plan: "01"
subsystem: test-scaffolding
tags: [tdd, red-stubs, resilience, sqlite, retry, pending-jobs, crash-recovery]
dependency_graph:
  requires: []
  provides: [retry_task_test.rs, pending_jobs_test.rs, config_store_test-v2-assertions]
  affects: [tests/retry_task_test.rs, tests/pending_jobs_test.rs, tests/config_store_test.rs]
tech_stack:
  added: []
  patterns:
    - "in-memory SQLite via rusqlite::Connection::open_in_memory() + execute_batch for test schema"
    - "one-shot HTTP stub via TcpListener::bind(127.0.0.1:0) copied from noren_client_test.rs"
    - "#[ignore = \"Wave N: reason\"] for forward-referencing not-yet-implemented functions"
key_files:
  created:
    - tests/retry_task_test.rs
    - tests/pending_jobs_test.rs
  modified:
    - tests/config_store_test.rs
decisions:
  - "Used test-schema without CHECK constraint in retry_task_test make_test_conn so any status value can be seeded freely (constraint tested in config_store_test)"
  - "CR-02 validate_job_id invariant tested at string level (not by calling the pub(crate) fn) — documents Wave 3 wiring requirement without requiring a public re-export"
  - "Wave-3 ignored tests written with full assertion bodies in comments so Wave 3 just removes #[ignore] and uncomments"
metrics:
  duration: "~4 minutes"
  completed: "2026-07-16"
  tasks_completed: 3
  tasks_total: 3
  files_created: 2
  files_modified: 1
---

# Phase 06 Plan 01: Wave-0 RED Test Stubs Summary

**One-liner:** Three test files scaffold the complete Phase 6 verification surface — two new RED test files covering RES-01/02/03/04 SQL invariants and HTTP contracts, plus updated config_store_test asserting user_version=2 for Wave 1's migration.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Create tests/retry_task_test.rs (RES-01/02/04) | `96e548e` | tests/retry_task_test.rs (created) |
| 2 | Create tests/pending_jobs_test.rs (RES-03) | `e4782c5` | tests/pending_jobs_test.rs (created) |
| 3 | Update config_store_test.rs user_version to 2 | `27a7bbc` | tests/config_store_test.rs (modified) |

## Test Outcome Summary

| File | Non-ignored | Ignored | Status |
|------|-------------|---------|--------|
| retry_task_test.rs | 3 (SQL invariant tests) | 2 (Wave 2 placeholders) | PASS |
| pending_jobs_test.rs | 1 (CR-02 doc test) | 3 (Wave 3 HTTP tests) | PASS |
| config_store_test.rs | 2 (unchanged) | 0 | 3 RED (intentional, Wave 1) |

## What Was Built

### tests/retry_task_test.rs (RES-01/02/04)

- `make_test_conn()`: in-memory schema-v2 (`printed_jobs` + `retry_queue`) without CHECK constraint for free seeding
- `crash_recovery_selects_orphaned_printing_rows`: asserts the D-05 orphan query returns exactly the row with status='printing' NOT in retry_queue (RES-04)
- `retry_poll_selects_due_rows_oldest_first`: asserts the D-06 poll query returns 2 due rows ordered oldest-first, excluding a future row (RES-01)
- `retry_queue_insert_stores_blob_bytes`: asserts D-12 INSERT stores ESC/POS bytes as BLOB that round-trip correctly; INSERT OR IGNORE deduplicates (RES-01)
- `retry_exhaustion_marks_failed` (#[ignore = "Wave 2"]): documents exhaustion invariant (RES-02)
- `retry_task_smoke` (#[ignore = "Wave 2"]): documents `run_retry_task` entry point (RES-01/02/04)

### tests/pending_jobs_test.rs (RES-03)

- `spawn_stub`: one-shot HTTP listener copied verbatim from noren_client_test.rs
- `fetch_pending_jobs_200_parses_wrapper` (#[ignore = "Wave 3"]): asserts `{"jobs":[...]}` wrapper-object shape with `jobId`/`type` camelCase keys (D-09 contract)
- `fetch_pending_jobs_empty_array` (#[ignore = "Wave 3"]): asserts empty jobs returns Ok (not Err)
- `fetch_pending_jobs_non_200_returns_err` (#[ignore = "Wave 3"]): asserts 500 returns Err
- `validate_job_id_rejects_traversal_documented` (active): asserts CR-02 security invariant at string level; documents Wave 3 wiring requirement

### tests/config_store_test.rs (migration v2 assertions)

- `test_schema_and_user_version` (renamed from `test_schema_v1_and_user_version`): now asserts user_version == 2 — RED until Wave 1 adds migration v2
- `test_open_and_migrate_idempotent`: both `v1` and `v2` assertions changed to expect 2 — RED until Wave 1
- `printed_jobs_accepts_printing_status` (new): INSERT status='printing' must succeed after v2 migration — RED until Wave 1
- `test_write_read` and `test_get_absent_key` unchanged

## Requirement Coverage

| Requirement | Tests |
|-------------|-------|
| RES-01 (retry 3x/30s) | crash_recovery_selects_orphaned_printing_rows, retry_poll_selects_due_rows_oldest_first, retry_queue_insert_stores_blob_bytes, retry_task_smoke (#[ignore]) |
| RES-02 (toast + red tray after exhaustion) | retry_exhaustion_marks_failed (#[ignore]), retry_task_smoke (#[ignore]) |
| RES-03 (pull pending on reconnect) | fetch_pending_jobs_200_parses_wrapper (#[ignore]), fetch_pending_jobs_empty_array (#[ignore]), fetch_pending_jobs_non_200_returns_err (#[ignore]), validate_job_id_rejects_traversal_documented (active) |
| RES-04 (crash recovery via 'printing' fence) | crash_recovery_selects_orphaned_printing_rows, printed_jobs_accepts_printing_status (RED) |

## Deviations from Plan

None - plan executed exactly as written.

## Threat Flags

None. This plan creates test files only; no network endpoints, auth paths, or production schema changes introduced.

## Self-Check: PASSED

Files exist:
- tests/retry_task_test.rs FOUND
- tests/pending_jobs_test.rs FOUND
- tests/config_store_test.rs FOUND (modified)

Commits verified:
- 96e548e FOUND (test(06-01): add RED stubs for RES-01/02/04 in retry_task_test.rs)
- e4782c5 FOUND (test(06-01): add RED stubs for RES-03 in pending_jobs_test.rs)
- 27a7bbc FOUND (test(06-01): update config_store_test.rs for migration v2)
