---
phase: 06-resilience
reviewed: 2026-07-16T00:00:00Z
depth: standard
files_reviewed: 8
files_reviewed_list:
  - src/config_store.rs
  - src/health_state.rs
  - src/lib.rs
  - src/main.rs
  - src/noren_client.rs
  - src/print_worker.rs
  - src/pusher/client.rs
  - src/retry_task.rs
findings:
  critical: 2
  warning: 7
  info: 4
  total: 13
status: issues_found
---

# Phase 6: Code Review Report

**Reviewed:** 2026-07-16
**Depth:** standard
**Files Reviewed:** 8
**Status:** issues_found

## Summary

Reviewed the Phase 6 (Resilience) changes: SQLite migration v2 adding the `'printing'`
status, a print-worker crash-recovery fence + `retry_queue` enqueue, the new 4th Tokio task
(`retry_task.rs`), and the Pusher-reconnect pending-jobs pull. The security posture on
path-traversal (`validate_job_id`) and token handling is solid — the token is consistently
passed only via `.bearer_auth()` and never interpolated into logs, and `validate_job_id`
correctly guards both the pending-pull insert and the URL-building `fetch`/`ack` calls.

The concerns are in the **concurrency layer across the 4 WAL connections** and in the
**retry state machine**. Two BLOCKERs: (1) `busy_timeout` is set on only 1 of the 4
SQLite connections, so the other three will return `SQLITE_BUSY` and silently drop state
transitions under contention; (2) the retry task and print worker can both drive
`print_raw()` on the same physical printer with no coordination, and the crash-recovery
re-queue opens a double-print window against the live print worker for the same `job_id`.
Several WARNINGs cover attempt-count accounting drift, an unbounded background-send task
spawn, and NULL-handling in the retry poll query.

## Critical Issues

### CR-01: `busy_timeout` set on only 1 of 4 WAL connections — silent `SQLITE_BUSY` drops state transitions

**File:** `src/retry_task.rs:58`, `src/main.rs:337`, `src/print_worker.rs:48`, `src/pusher/client.rs:158`
**Issue:**
Four SQLite connections are opened in WAL mode: main App (`main.rs:337`), Pusher task
(`pusher/client.rs:158`), print worker (`print_worker.rs:48`), and retry task
(`retry_task.rs:52`). Under WAL, reads never block writers but **writers are still
serialized — only one writer may hold the write lock at a time.** Only the retry task
calls `conn.busy_timeout(Duration::from_secs(5))` (`retry_task.rs:58`). The other three
connections have **no busy timeout**, so the moment two of them attempt a write
concurrently, the loser gets `SQLITE_BUSY` returned *immediately* (default timeout is 0).

Concretely: the retry task holds the write lock inside its poll loop (`UPDATE
printed_jobs SET status='printing'`, then later `UPDATE ... 'printed'`/`'failed'` + `DELETE
FROM retry_queue`). If the print worker simultaneously runs its `UPDATE ... status='printing'`
fence (`print_worker.rs:143`) or its `INSERT ... retry_queue` (`print_worker.rs:167`), that
write returns `SQLITE_BUSY`. In the print worker those failures are only logged and the loop
`continue`s or proceeds — the crash-recovery fence is silently skipped, or the `retry_queue`
INSERT is dropped, meaning **a failed print is never enqueued for retry** (a lost comanda —
the core reliability guarantee). The Pusher task's `insert_print_job` can likewise fail,
skipping the mpsc send entirely.

This directly contradicts the stated invariant ("nenhuma comanda perdida").

**Fix:** Set `busy_timeout` on **every** connection immediately after enabling WAL, not just
the retry task's:
```rust
// After each `pragma_update(None, "journal_mode", "WAL")`:
let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
```
Apply to `main.rs` (after line 338), `print_worker.rs` (after line 51), and
`pusher/client.rs` (after line 161). Consider a shared `open_wal_connection(path)` helper in
`config_store.rs` so all four call sites share identical PRAGMA setup and cannot drift.

### CR-02: Crash-recovery re-queue opens a double-print / concurrent `print_raw` window for the same `job_id`

**File:** `src/retry_task.rs:69-110`, `src/main.rs:432-451`
**Issue:**
The print worker and retry task each construct their **own** `Box<dyn Printer>`
(`main.rs:451` and `print_worker.rs:84`) and the comment at `main.rs:433-434` asserts "two
concurrent calls are safe." That is only true if the two tasks never target the same job at
the same instant — but crash recovery breaks that assumption.

Trace: the print worker sets `status='printing'` (`print_worker.rs:143`) **before** calling
`print_raw` (`print_worker.rs:159`). This intermediate state is durable. The retry task's
startup crash-recovery query (`retry_task.rs:70-73`) selects **all** `status='printing'`
rows not yet in `retry_queue`. Because the print worker leaves the row at `'printing'` for
the entire duration of a *successful* print (it only flips to `'printed'` at
`print_worker.rs:188`, after `print_raw` returns), a retry task that starts up — or that
runs its recovery scan — while the print worker is mid-print on job X will see job X as an
orphan, re-fetch its bytes (`retry_task.rs:91`), and INSERT it into `retry_queue`. On the
next poll tick it will call `printer.print_raw(&escpos_bytes)` for job X **while the print
worker's `print_raw` for the same job may still be in flight** → the comanda prints twice,
and two concurrent raw handles hit the same USB/serial device.

The recovery query cannot distinguish "crashed while printing" from "currently printing in
another live task" — both look identical (`status='printing'`, not in `retry_queue`). Since
crash recovery runs not only at cold start but is the documented startup behavior of a task
that is spawned concurrently with the print worker (`main.rs:496-512`), the two tasks race on
first launch.

**Fix:** Gate crash recovery on a durable "the previous process is gone" signal rather than
on `status='printing'` alone. Options:
- Add an age guard: only treat a `'printing'` row as an orphan if `received_at`/a fence
  timestamp is older than a threshold (e.g. `printed_at IS NULL AND <fence_ts> < now-60s`),
  so an in-flight print is never re-queued.
- Or record the owning PID / process-start epoch on the `'printing'` fence and only reclaim
  rows whose owner is not the current process.
- At minimum, do crash recovery **once at startup before the print worker task is spawned**,
  and never let it run concurrently with a live worker on the same `job_id`.

## Warnings

### WR-01: Retry attempt accounting yields 4 total print attempts, and the exhaustion log is off-by-one

**File:** `src/retry_task.rs:156-222`, `src/print_worker.rs:171`
**Issue:**
The print worker enqueues with `attempt_count=1` (`print_worker.rs:171`) after its own
`print_raw` already failed once. The retry poll then reads `attempt_count` and branches
`Err(e) if attempt_count < 3` (`retry_task.rs:192`). Walking the states:
original worker print (fail) → enqueue `attempt_count=1` → retry reads 1, prints (fail),
`1<3` → set 2 → retry reads 2, prints (fail), `2<3` → set 3 → retry reads 3, prints (fail),
`3<3` false → exhausted. That is **3 retry-task print attempts** (4 including the worker's
original), which matches "3× retry." However the exhaustion log at `retry_task.rs:208-209`
says "exhausted after 3 attempts" while the failing attempt just logged was the one where
`attempt_count == 3` — and the per-attempt log at `retry_task.rs:201-203` prints
`attempt {attempt_count}` using the *pre-increment* value, so the operator sees attempts
logged as "1", "2" and then a jump to "exhausted after 3" with no "attempt 3" line. The
counting is defensible but the logging is inconsistent and will mislead debugging of the
core reliability path.

**Fix:** Make the log reflect the actual attempt number consistently, e.g. log
`attempt {attempt_count} of 3` on each failure including the exhausting one, or increment
before logging. Add a comment on the `attempt_count=1` seed explaining the "+1 for the
worker's original print" offset so the `< 3` threshold is not later "corrected" to `<= 3`.

### WR-02: `escpos_bytes` NULL / empty not guarded in retry poll — a NULL blob row loops forever

**File:** `src/retry_task.rs:126-154`, `src/print_worker.rs:168-172`
**Issue:**
`retry_queue.escpos_bytes` is a nullable `BLOB` (`config_store.rs:57`). The poll query
binds it as `row.get::<_, Vec<u8>>(2)?` (`retry_task.rs:144`). If any row ever has
`escpos_bytes = NULL` (e.g. a future enqueue path, or a partially-written row), `get::<Vec<u8>>`
returns an `InvalidColumnType` error, the whole `query_map` row is dropped by
`filter_map(|r| r.ok())` (`retry_task.rs:148`), and the row is **never processed, never
retried, and never exhausted** — it sits in `retry_queue` permanently while `next_retry_at`
stays in the past, contributing nothing but a silent stuck job. There is no path that marks
it failed.

**Fix:** Read as `Option<Vec<u8>>` and, when `None` (or empty), mark the job `'failed'` and
`DELETE` it from `retry_queue` rather than silently skipping it. Alternatively add
`escpos_bytes BLOB NOT NULL` to the schema if a NULL is never legitimate.

### WR-03: Unbounded `tokio::spawn` background sends can reorder jobs and grow without limit

**File:** `src/pusher/client.rs:324-327`, `src/pusher/client.rs:411-414`
**Issue:**
When the print channel is `Full`, both the pending-pull path and the live-event path
`tokio::spawn` a detached task that does `tx.send(ev).await` (`client.rs:325-327` and
`411-414`). Under a sustained backlog this spawns an **unbounded** number of detached tasks
(no join handle, no cap), and because each detached task completes whenever the channel
drains, the jobs can be delivered to the worker **out of order** relative to both each other
and the inline `try_send` path. For a print queue where comanda ordering matters (kitchen
tickets), silent reordering is a correctness concern, and the unbounded spawn is a latent
resource issue on a stuck worker.

**Fix:** Replace the detached spawn with a single bounded overflow strategy — e.g. increase
the mpsc capacity, or use a dedicated ordered forwarding task with one `send().await`
inline, or `send_timeout` with a bounded wait. At minimum cap concurrent overflow sends and
preserve FIFO ordering.

### WR-04: Retry success/exhaustion ignores `UPDATE`/`DELETE` return values — stuck or duplicate rows go undetected

**File:** `src/retry_task.rs:176-188`, `src/retry_task.rs:211-218`
**Issue:**
The success branch and exhaustion branch use `let _ = conn.execute(...)` for the
`UPDATE printed_jobs`, `DELETE FROM retry_queue`, and reschedule UPDATEs
(`retry_task.rs:176`, `185`, `195`, `211`, `215`). Unlike the print worker — which checks
`Ok(0)` to detect "row absent" (`print_worker.rs:146`, `191`) — the retry task discards all
outcomes. If the `DELETE FROM retry_queue` after a successful print silently affects 0 rows
(or errors due to CR-01's `SQLITE_BUSY`), the job stays in `retry_queue` and will be
**re-printed on the next poll** — a duplicate comanda. If the exhaustion `UPDATE ... 'failed'`
fails, the row is deleted from the queue but left `'printing'`, becoming a permanent orphan
that crash recovery re-queues on every subsequent boot.

**Fix:** Check the `execute` results (at least `Ok(0)` and `Err`) on the success `DELETE`
and the exhaustion `UPDATE`, logging when the expected row count is not met. Given CR-01,
these failures are not hypothetical.

### WR-05: `printer_type` defaulting silently routes any non-`"serial"` value to Spooler

**File:** `src/main.rs:442-449`, `src/print_worker.rs:74-81`
**Issue:**
Both the retry task setup (`main.rs:445`) and the print worker (`print_worker.rs:77`) treat
`printer_type` as serial only on an exact `== "serial"` match and route **everything else**
— including a missing key, an empty string, or a typo/corrupted value — to
`PrinterId::Spooler`. If `printer_type` is corrupted or an unexpected value, the agent
silently tries to print a serial (COM) device through the Windows spooler by name, which
will fail every print and drive the job straight to the retry/exhaustion path with a
confusing error. There is no validation that `printer_type` is one of the two known values.

**Fix:** Match explicitly on `"serial" | "spooler"` and log/warn on any other value rather
than defaulting silently. Consider persisting `printer_type` as a validated enum at
activation time.

### WR-06: Print-worker `'printing'` fence increments `attempt`, but a fetch-failure retry never decrements — attempt inflation

**File:** `src/print_worker.rs:142-155`, `src/pusher/client.rs:299-358`
**Issue:**
The print worker's `'printing'` fence does `attempt=attempt+1` (`print_worker.rs:143`)
*after* a successful byte fetch but *before* `print_raw`. On the RES-03 pending-pull path
(`client.rs:299`), the same `job_id` can be re-delivered to the worker on every Pusher
reconnect (the `insert_print_job` dedup only blocks the *first* INSERT; once a row exists at
`status='printing'` or `'pending'`, a reconnect that re-pulls the job and finds it already
in `printed_jobs` returns `Ok(false)` and skips the send — good — but the *live* `print:job`
event path and the pending path both drive the same worker). Each real fetch+fence pass
bumps `attempt` unbounded with no ceiling and no relation to the retry_queue's separate
`attempt_count`. The two counters (`printed_jobs.attempt` vs `retry_queue.attempt_count`)
track different things and neither bounds the worker's own re-processing. This is minor
today but the dual-counter design is fragile and undocumented at the field level.

**Fix:** Document the distinction between `printed_jobs.attempt` (worker fence increments)
and `retry_queue.attempt_count` (retry-task increments) at the schema (`config_store.rs`),
and confirm the worker cannot re-fence an already-`'printed'` row (add
`WHERE job_id=?1 AND status != 'printed'` to the fence UPDATE to prevent reviving a completed
job).

### WR-07: `pusher_conn` in the pending pull writes without WAL busy-timeout — same silent-drop class as CR-01

**File:** `src/pusher/client.rs:310`, `src/pusher/client.rs:394`
**Issue:**
`insert_print_job(&pusher_conn, ...)` writes to `printed_jobs` from the Pusher connection,
which (per CR-01) has no `busy_timeout`. If it loses the write lock to the retry task or
print worker, `Err(e)` is logged (`client.rs:344-347`, `430-431`) and the mpsc send is
skipped — the job is neither persisted nor forwarded to the worker, and the RES-03 recovery
guarantee relies on the *next* reconnect re-pulling it. This is a specific instance of CR-01
but called out separately because it silently defeats the offline-recovery pending pull,
which is the headline Phase 6 feature.

**Fix:** Covered by CR-01's busy_timeout fix; additionally, on `insert_print_job` `Err`,
do not lose the job — the next pending pull will re-fetch it, which is acceptable, but log at
a severity that makes the drop visible.

## Info

### IN-01: Duplicate printer construction and config reads between `main.rs` and `print_worker.rs`

**File:** `src/main.rs:438-451`, `src/print_worker.rs:64-84`
**Issue:** `printer_name`/`printer_type` are read and a `PrinterId` constructed in two
places with near-identical logic (main.rs for the retry task, print_worker.rs for the
worker). Divergence risk if one is later changed (e.g. WR-05 fix applied to only one).
**Fix:** Extract a shared `fn printer_id_from_config(conn) -> Option<PrinterId>` helper.

### IN-02: Stale/misleading startup log — always prints `user_version=1`

**File:** `src/main.rs:332`
**Issue:** After the v2 migration lands, `open_and_migrate` advances `user_version` to 2,
but the log line hardcodes `"state.db migrated (user_version=1)"`. Misleading during
debugging of migration issues.
**Fix:** Read and print the actual `user_version` pragma, or drop the hardcoded number.

### IN-03: Migration v2 relies on positional `INSERT INTO ... SELECT *` column-order coupling

**File:** `src/config_store.rs:85`
**Issue:** `INSERT INTO printed_jobs_v2 SELECT * FROM printed_jobs` copies rows positionally.
This is correct today (v1 and v2 column order match exactly), but any future reorder of the
v1 column list would silently corrupt data with no error. The comment notes the coupling but
the SQL does not enforce it.
**Fix:** Use an explicit column list in both the INSERT target and the SELECT:
`INSERT INTO printed_jobs_v2 (job_id, job_type, status, attempt, received_at, printed_at,
failed_at) SELECT job_id, job_type, status, attempt, received_at, printed_at, failed_at
FROM printed_jobs;`

### IN-04: Test schemas drift from production schema (missing `'printing'` CHECK and FK)

**File:** `src/pusher/client.rs:498-513`, `src/retry_task.rs:261-285`
**Issue:** The in-memory test schemas omit the `status` CHECK constraint and the
`retry_queue.job_id REFERENCES printed_jobs(job_id)` FK present in production
(`config_store.rs:53-62`, `74-88`). Tests therefore cannot catch a status value or FK
violation that production would reject, weakening the value of the retry/dedup tests.
**Fix:** Have the tests run the real `MIGRATIONS.to_latest()` against the in-memory
connection instead of hand-rolling a divergent schema.

---

_Reviewed: 2026-07-16_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
