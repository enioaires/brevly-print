---
phase: 06-resilience
plan: "02"
subsystem: config-store + print-worker
tags: [resilience, crash-recovery, retry-queue, sqlite-migration, resque-04, resque-01]
dependency_graph:
  requires: ["06-01"]
  provides: ["06-03", "06-04"]
  affects: ["src/config_store.rs", "src/print_worker.rs"]
tech_stack:
  added: []
  patterns:
    - "SQLite table-recreation migration pattern (CREATE v2, INSERT SELECT *, DROP, RENAME, CREATE INDEX)"
    - "Crash-recovery fence: UPDATE status='printing' before print_raw()"
    - "INSERT OR IGNORE INTO retry_queue with BLOB binding via bytes.as_slice()"
key_files:
  modified:
    - src/config_store.rs
    - src/print_worker.rs
decisions:
  - "D-01: Migration v2 uses table-recreation (not ALTER TABLE) because SQLite cannot modify CHECK constraints in place"
  - "D-02: status='printing' fence UPDATE precedes print_raw(); attempt incremented here so success UPDATE does not double-count"
  - "D-11: No send_health added to run_print_worker — retry task owns health state transitions"
  - "D-12: INSERT OR IGNORE (not INSERT) prevents double-insert if same job_id arrives twice in mpsc channel"
metrics:
  duration: "4m"
  completed: "2026-07-16T19:25:09Z"
  tasks_completed: 2
  tasks_total: 2
  files_modified: 2
---

# Phase 06 Plan 02: Crash Recovery Foundation Summary

**One-liner:** SQLite migration v2 expands printed_jobs CHECK to include 'printing' status, and the print worker sets that fence before print_raw() then enqueues failed jobs to retry_queue with their ESC/POS bytes as a BLOB.

## What Was Built

### Task 1: Migration v2 in config_store.rs (d94d3ce)

Added a second `M::up` entry to the `MIGRATIONS` vec in `src/config_store.rs`. The migration uses the standard SQLite table-recreation pattern (the only way to modify a CHECK constraint in SQLite):

1. `CREATE TABLE printed_jobs_v2` with expanded CHECK: `('pending','printing','printed','failed')`
2. `INSERT INTO printed_jobs_v2 SELECT * FROM printed_jobs` — positional column copy preserving all v1 rows
3. `DROP TABLE printed_jobs`
4. `ALTER TABLE printed_jobs_v2 RENAME TO printed_jobs`
5. `CREATE INDEX idx_printed_jobs_status ON printed_jobs(status)` — re-create index on renamed table

No `PRAGMA foreign_keys` added (rusqlite_migration disables FK enforcement during migrations by default, making the DROP/RENAME safe despite `retry_queue.job_id REFERENCES printed_jobs(job_id)`).

Updated module doc comment to note that v2 adds the 'printing' intermediate status for crash recovery.

`user_version` advances to 2 after migration; all three config_store_test assertions turn GREEN.

### Task 2: status='printing' fence + retry_queue INSERT in print_worker.rs (4a51688)

Two surgical edits to the enabled-type printing path:

**EDIT 1 — D-02 fence (before print_raw):** After `fetch_job_bytes` success and before `printer.print_raw(&bytes)`, a new UPDATE sets the crash-recovery fence:
```sql
UPDATE printed_jobs SET status='printing', attempt=attempt+1 WHERE job_id=?1
```
Uses the same three-arm match style as the existing success UPDATE. The fence also increments `attempt` so the success path UPDATE no longer needs to (preventing double-increment).

**EDIT 2 — D-12 retry_queue INSERT (on print_raw failure):** The old `continue; // leave status='pending'` is replaced with:
1. Capture `error_msg = e.to_string()`
2. `INSERT OR IGNORE INTO retry_queue` with `bytes.as_slice()` as the BLOB parameter, `attempt_count=1`, `next_retry_at = now + 30s`
3. Log if the INSERT fails
4. `continue` — status stays 'printing' (intentional, do NOT revert to 'pending')

**Comment updates:**
- Fetch failure comment updated: "leave status='pending' — no bytes fetched; RES-03 pending pull re-fetches on reconnect"
- Success path UPDATE comment updated: notes attempt was already incremented by the fence
- `attempt=attempt+1` removed from success UPDATE SQL (only remains in the fence and the disabled-type branch which has its own independent path)

## Verification Results

All verification criteria passed:

| Test Suite | Result |
|-----------|--------|
| `cargo test --test config_store_test` | 5/5 PASS (user_version==2, printing accepted, tables exist, idempotent, write-read, absent-key) |
| `cargo test --test print_worker_test` | 6/6 PASS (existing filter/ordering tests unchanged) |
| `cargo test retry_queue_insert` | 1/1 PASS |
| `cargo build` | SUCCESS (0 crates recompiled) |

## Deviations from Plan

None — plan executed exactly as written.

## Threat Surface Scan

| Flag | File | Description |
|------|------|-------------|
| T-06-02 (mitigated) | src/print_worker.rs | `error_msg` stored in retry_queue.last_error — verified agent_token is never in scope in the print_raw failure path (T-02-02 satisfied) |

No new threat surface introduced beyond what the plan's threat model already registers.

## Known Stubs

None — both files produce real side effects (SQLite migration runs once at startup; retry_queue INSERT stores real bytes). No placeholder data or TODO stubs.

## Self-Check: PASSED

Files exist:
- FOUND: src/config_store.rs (modified)
- FOUND: src/print_worker.rs (modified)

Commits exist:
- d94d3ce — feat(06-02): add migration v2
- 4a51688 — feat(06-02): add status='printing' fence + retry_queue INSERT
