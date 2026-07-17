---
phase: 05-job-pipeline
fixed_at: 2026-07-16T18:30:00Z
review_path: .planning/phases/05-job-pipeline/05-REVIEW.md
iteration: 1
findings_in_scope: 8
fixed: 8
skipped: 0
status: all_fixed
---

# Phase 05: Code Review Fix Report

**Fixed at:** 2026-07-16T18:30:00Z
**Source review:** .planning/phases/05-job-pipeline/05-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 8 (CR-01, CR-02, CR-03, WR-01, WR-02, WR-03, WR-04, WR-05)
- Fixed: 8
- Skipped: 0

## Fixed Issues

### CR-01: `unwrap_err()` Panics When Server Returns 201 with Valid JSON Body

**Files modified:** `src/noren_client.rs`
**Commit:** 9cd2294
**Applied fix:** Added `ActivateError::UnexpectedStatus(u16)` variant to the enum with an
`#[error("Resposta inesperada do servidor: HTTP {0}")]` display string. Replaced the
`201..=299` match arm that called `.unwrap_err()` with a safe branch that drains the body
(connection reuse) and returns `Err(ActivateError::UnexpectedStatus(status))`. The
contradictory comment was updated to describe the new implementation.

---

### CR-02: `job_id` from Pusher Payload Interpolated Into HTTP URL Without Validation (Path Traversal)

**Files modified:** `src/noren_client.rs`
**Commit:** e7fd50a
**Applied fix:** Added a private `validate_job_id(job_id: &str) -> anyhow::Result<()>` function
that rejects empty IDs and IDs containing `/`, `.`, `\`, `?`, `#`, `%`, or NUL. Applied the
validation call at the entry of both `fetch_job_bytes` and `ack_job`, before the `format!`
URL construction. A crafted job_id can now only cause the job to be skipped with an error
log rather than issuing a path-traversal request to the Noren backend.

---

### CR-03: C4 Ordering Test Does Not Protect the Success-Path UPDATE-before-ack Constraint

**Files modified:** `tests/print_worker_test.rs`
**Commit:** afc8c06
**Applied fix:** Rewrote `update_precedes_ack_in_source()` to check both code paths
independently. The success path now uses `rfind()` for both the UPDATE and ack_job strings
(capturing the last/deepest pair). The disabled-type path uses `find()` for both (capturing
the first/earliest pair). Both assertions fire independently, so a refactor that swaps ack
before UPDATE on either path will correctly fail the test.

---

### WR-01: `ack_job` Does Not Accept HTTP 204 as Success

**Files modified:** `src/noren_client.rs`
**Commit:** be3c2dd
**Applied fix:** Added `204` to the success match arm in `ack_job`: `200 | 204 | 409 => Ok(())`.

---

### WR-02: Disabled-Type Branch Continues to Ack Even When SQLite UPDATE Fails

**Files modified:** `src/print_worker.rs`
**Commit:** d8cf189
**Applied fix:** Replaced the `if let Err(e) =` pattern in the disabled-type UPDATE with a
`match` expression. On `Err(e)`, the branch now `continue`s — skipping the ack and leaving
the row at `status='pending'` for Phase 6 retry. Previously the code acked regardless of
whether the UPDATE succeeded, which could leave the row permanently stuck at `pending`.

Note: WR-02, WR-03, and WR-05 were applied in a single commit because all three modify the
same `conn.execute()` call patterns in `print_worker.rs`.

---

### WR-03: `attempt` Column Is Never Incremented — Retry Counter Is Permanently Zero

**Files modified:** `src/print_worker.rs`
**Commit:** d8cf189
**Applied fix:** Added `attempt=attempt+1` to both UPDATE statements (disabled-type path and
success path). Phase 6 retry logic that reads `attempt` for backoff or give-up logic will
now see accurate attempt counts.

---

### WR-04: `tx.send(event).await` Inside `tokio::select!` Arm Can Starve the Ping Timer

**Files modified:** `src/pusher/client.rs`
**Commit:** c47f515
**Applied fix:** Replaced `let _ = tx.send(event).await` with `tx.try_send(event)`. On
`TrySendError::Full`, logs the backpressure event and spawns a background task that does the
blocking `tx2.send(ev).await`, so the `select!` loop is never blocked and the ping timer
continues to be polled. On `TrySendError::Closed`, logs and breaks the inner loop. The ping
zombie detection is now active even under channel backlog bursts.

---

### WR-05: `UPDATE` Rows-Affected Count Is Silently Discarded — Zero-Row Updates Go Unlogged

**Files modified:** `src/print_worker.rs`
**Commit:** d8cf189
**Applied fix:** Converted both `if let Err(e) = conn.execute(...)` patterns to `match`
expressions with an explicit `Ok(0) =>` arm that logs when zero rows were affected. This
catches the case where `job_id` is absent from `printed_jobs` (the Pusher task's
`INSERT OR IGNORE` may have silently failed) and the job is acked with no DB record.

---

## Skipped Issues

None — all findings were fixed.

---

_Fixed: 2026-07-16T18:30:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
