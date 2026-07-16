---
phase: 05-job-pipeline
reviewed: 2026-07-16T17:52:07Z
depth: standard
files_reviewed: 6
files_reviewed_list:
  - src/lib.rs
  - src/main.rs
  - src/noren_client.rs
  - src/printer/mod.rs
  - src/print_worker.rs
  - tests/print_worker_test.rs
findings:
  critical: 3
  warning: 5
  info: 2
  total: 10
status: issues_found
---

# Phase 05: Code Review Report

**Reviewed:** 2026-07-16T17:52:07Z
**Depth:** standard
**Files Reviewed:** 6
**Status:** issues_found

## Summary

Reviewed the Phase 5 job pipeline: the print worker task (`src/print_worker.rs`),
Noren HTTP client endpoints for job fetch and ack (`src/noren_client.rs`), the printer
abstraction layer (`src/printer/mod.rs`), startup wiring in `main.rs`, the library
root `lib.rs`, and the integration test suite. The Pusher client was read as supporting
context to trace the full `job_id` data flow.

The core job pipeline structure is sound: the fetch-print-UPDATE-ack sequence is
correctly ordered on all code paths, the dedup fence (`INSERT OR IGNORE`) works
correctly, and WAL mode is applied consistently. Three blockers were found: a
panic-on-success in `noren_client::activate()`, an unvalidated `job_id` string used
directly in URL path construction (path traversal), and a critical flaw in the C4
ordering test that will silently pass even when the success-path ordering constraint
is broken. Five warnings and two info items cover lesser issues.

---

## Critical Issues

### CR-01: `unwrap_err()` Panics When Server Returns 201 with Valid JSON Body

**File:** `src/noren_client.rs:139-141`

**Issue:** The `activate()` function handles `201..=299` responses with:

```rust
201..=299 => Err(ActivateError::Transport(
    resp.json::<ActivateResponse>().await.unwrap_err(),
)),
```

The comment on lines 136-138 says the JSON parse will fail "without calling
`unwrap_err`" — but the code immediately calls `.unwrap_err()`. If the Noren server
returns HTTP 201 with a body that is syntactically valid `ActivateResponse` JSON (some
REST frameworks return `201 Created` with the same response body as `200 OK`),
`.json().await` succeeds and returns `Ok(response)`. Calling `.unwrap_err()` on `Ok`
panics the process:

```
called Result::unwrap_err() on an Ok value: ActivateResponse { ... }
```

This crashes the tray agent during the activation flow with no recovery. The
contradictory comment shows that the implementation was edited after the comment was
written and the invariant no longer holds.

**Fix:** Replace the `unwrap_err()` branch with a safe alternative. Minimal fix:

```rust
201..=299 => {
    let status = resp.status().as_u16();
    let _ = resp.bytes().await; // drain body to allow connection reuse
    Err(ActivateError::Transport(
        // Build a synthetic reqwest error by issuing a known-bad local request
        // and mapping its error, OR add ActivateError::UnexpectedStatus(u16).
        // Simplest clean fix: add a new variant to ActivateError:
        // ActivateError::UnexpectedStatus(u16)
        // Until then, InvalidSerial is wrong but avoids the panic:
        panic!("placeholder — replace with ActivateError::UnexpectedStatus({status})")
    ))
}
```

Recommended fix — add an `UnexpectedStatus` variant to `ActivateError`:

```rust
#[error("Resposta inesperada do servidor: HTTP {0}")]
UnexpectedStatus(u16),
```

And use it:

```rust
201..=299 => {
    let status = resp.status().as_u16();
    let _ = resp.bytes().await;
    Err(ActivateError::UnexpectedStatus(status))
}
```

---

### CR-02: `job_id` from Pusher Payload Interpolated Into HTTP URL Without Validation (Path Traversal)

**File:** `src/noren_client.rs:244`, `src/noren_client.rs:282`

**Issue:** Both `fetch_job_bytes` and `ack_job` construct request URLs by directly
string-interpolating the caller-supplied `job_id`:

```rust
// fetch_job_bytes
let url = format!("{base_url}/api/agent/jobs/{job_id}/bytes");

// ack_job
let url = format!("{base_url}/api/agent/jobs/{job_id}/ack");
```

`job_id` originates from the Pusher WebSocket payload: `PusherEnvelope.data` is
deserialized as `PrintEvent.job_id` (a plain `String` with no format constraints).
`reqwest` does not percent-encode or sanitize path segment characters in format-
interpolated URL strings. A crafted `job_id` such as `"../admin"` or
`"x%2F..%2Fadmin"` or `"foo/../../other-tenant/bar"` produces requests to
unintended backend endpoints.

The Pusher channel is subscriber-authenticated, but if the Pusher channel is
compromised, or if the Noren backend itself sends a crafted `job_id` (confused-deputy
scenario), the agent will forward path-traversal requests to the Noren server. The
Noren backend may also expose other tenant data or internal routes at paths reachable
through traversal.

**Fix:** Validate `job_id` before any URL construction. Reject IDs containing `/`,
`.`, `\`, `?`, `#`, or `%` (path, query, fragment, and encoding special characters):

```rust
fn validate_job_id(job_id: &str) -> anyhow::Result<()> {
    if job_id.is_empty() {
        anyhow::bail!("job_id is empty");
    }
    if job_id.chars().any(|c| matches!(c, '/' | '.' | '\\' | '?' | '#' | '%' | '\0')) {
        anyhow::bail!("job_id contains invalid characters: {job_id:?}");
    }
    Ok(())
}
```

Apply this validation at the top of both `fetch_job_bytes` and `ack_job`, before
`format!`. Alternatively, apply it at the `PrintEvent` deserialization site in
`pusher/client.rs` so malformed events are rejected before they reach the channel.

---

### CR-03: C4 Ordering Test Does Not Protect the Success-Path UPDATE-before-ack Constraint

**File:** `tests/print_worker_test.rs:139-156`

**Issue:** `update_precedes_ack_in_source` uses `.find()` (first occurrence) for
`UPDATE` and `.rfind()` (last occurrence) for `ack_job(`. There are **two** UPDATE
statements in `print_worker.rs`:

- Byte 3888: inside the **disabled-type** branch (line 93)
- Byte 5807: inside the **success** branch (line 138)

And **two** `ack_job(&` call sites:

- Byte 4314: inside the **disabled-type** branch (line 102)
- Byte 6375: inside the **success** branch (line 150)

The test computes `update_idx = 3888` (disabled-type UPDATE) and
`ack_idx = 6375` (success-path ack). The assertion `3888 < 6375` is always true and
does not validate the success-path ordering at all.

Proof: if the success-path ack (currently at 6375) were refactored to appear at byte
5500 — before the success-path UPDATE at 5807 — the violation is:

```
success UPDATE at 5807, success ack at 5500 → ack fires BEFORE UPDATE (C4 broken)
```

But the test still asserts `3888 (disabled UPDATE) < 5500 (swapped success ack)`,
which is `True`. The test **passes** despite the C4 constraint being broken on the
critical success path.

**Fix:** Use `.rfind()` for both to capture the last (success-path) occurrence of
each, and additionally assert the disabled-type path using `.find()` for both:

```rust
fn update_precedes_ack_in_source() {
    let src = include_str!("../src/print_worker.rs");

    // Success path: rfind finds the LAST occurrence of each (success-path pair).
    let success_update = src
        .rfind("UPDATE printed_jobs SET status='printed'")
        .expect("success-path UPDATE not found");
    let success_ack = src
        .rfind("ack_job(")
        .expect("success-path ack_job not found");
    assert!(
        success_update < success_ack,
        "C4 violated (success path): UPDATE ({success_update}) must precede ack_job ({success_ack})"
    );

    // Disabled-type path: find the FIRST occurrence of each.
    let dt_update = src
        .find("UPDATE printed_jobs SET status='printed'")
        .unwrap();
    let dt_ack = src.find("ack_job(").unwrap();
    assert!(
        dt_update < dt_ack,
        "C4 violated (disabled-type path): UPDATE ({dt_update}) must precede ack_job ({dt_ack})"
    );
}
```

---

## Warnings

### WR-01: `ack_job` Does Not Accept HTTP 204 as Success

**File:** `src/noren_client.rs:292`

**Issue:** The match arm for `ack_job` treats only `200` and `409` as success:

```rust
200 | 409 => Ok(()),
status => anyhow::bail!("ack_job: unexpected status {status}"),
```

REST convention allows `204 No Content` for acknowledgement endpoints with no
response body. If the Noren backend ever returns 204 — or is updated to do so — every
ack call will fail with an error log. Because the print worker continues past ack
failures (leaving `status='printed'` in SQLite), this is non-fatal today, but the
persistent error log will mask the issue. Phase 6 retry logic that re-examines
un-acked jobs may also misbehave.

**Fix:**

```rust
200 | 204 | 409 => Ok(()),
```

---

### WR-02: Disabled-Type Branch Continues to Ack Even When SQLite UPDATE Fails

**File:** `src/print_worker.rs:92-108`

**Issue:** In the disabled-type branch, if `conn.execute(UPDATE ...)` fails, the code
logs the error and then still calls `ack_job`. If the agent restarts (and the UPDATE
truly failed), the Pusher dedup fence (`INSERT OR IGNORE`) will not fire because the
`job_id` row already exists with `status='pending'`. The Pusher task re-delivers the
event (after reconnect or Pusher re-push), but the print worker's dedup fence only
runs in the Pusher task — the print worker itself has no dedup logic. However if
Pusher never re-delivers (because the ack succeeded), the row stays `pending` with no
path to transition it.

The comment at line 100 reads "Still attempt ack — Noren won't resend anyway." This
is only true if Noren's server-side queue dequeues on ack. If the UPDATE failed and
the ack also later fails (line 102-107), the job is permanently stuck at `pending`
with no mechanism to advance it (Phase 6 pending-pull is future work).

**Fix:** On UPDATE failure in the disabled-type path, skip the ack and leave the row
at `pending`, consistent with the error recovery strategy used in the success path:

```rust
if let Err(e) = conn.execute(...) {
    eprintln!("[brevly-print] Print worker: SQLite update failed for {}: {e:#}", event.job_id);
    continue; // skip ack — leave status='pending' for Phase 6 retry
}
if let Err(e) = ack_job(...).await {
    eprintln!("[brevly-print] Print worker: ack failed (disabled type) for {}: {e:#}", event.job_id);
}
```

---

### WR-03: `attempt` Column Is Never Incremented — Retry Counter Is Permanently Zero

**File:** `src/print_worker.rs:92-108` and `src/print_worker.rs:137-146`

**Issue:** The `printed_jobs` schema (in `config_store.rs`) defines an `attempt`
column (`INTEGER NOT NULL DEFAULT 0`). Neither UPDATE statement in `print_worker.rs`
increments it:

```rust
"UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1"
```

Every printed job will show `attempt=0` in the database regardless of how many times
the job was retried or reprocessed. Phase 6 retry logic that reads `attempt` to apply
exponential backoff or to give up after N retries cannot function correctly when the
counter is always zero.

**Fix:** Increment `attempt` in both UPDATE paths:

```rust
"UPDATE printed_jobs
    SET status='printed',
        printed_at=datetime('now'),
        attempt=attempt+1
 WHERE job_id=?1"
```

---

### WR-04: `tx.send(event).await` Inside `tokio::select!` Arm Can Starve the Ping Timer

**File:** `src/pusher/client.rs:331`

**Issue:** In the inner reconnect loop, the `print:job` branch calls
`tx.send(event).await` while holding the `ws.next()` arm of a `tokio::select!`. The
mpsc channel has a fixed capacity of 32 (`main.rs:394`). If the print worker falls
behind and the channel is full, `tx.send(event).await` blocks indefinitely inside the
select arm. While blocked there, the `ping_timer.tick()` arm is not polled. If the
send stays blocked longer than 30 seconds:

- The ping timer fires but is not polled → `awaiting_pong` is never set → no ping
  is sent → zombie detection is disabled for the duration of the stall.
- When the send eventually completes (print worker drains one slot), the next
  `ping_timer.tick()` has accumulated multiple ticks. With
  `MissedTickBehavior::Delay`, only one fires, but the ping was still missed for
  > 30 s.

Under a burst of 32+ rapid jobs (e.g., end-of-day batch), a zombie connection could
go undetected for the entire burst duration.

**Fix:** Use `try_send` and log on backpressure, or use a non-blocking send with a
timeout:

```rust
Ok(true) => {
    match tx.try_send(event) {
        Ok(()) => {}
        Err(tokio::sync::mpsc::error::TrySendError::Full(ev)) => {
            eprintln!(
                "[brevly-print] Pusher: print channel full — event {} queued with blocking send",
                ev.job_id
            );
            // Fall back to blocking send in a spawned task to avoid stalling the loop.
            let tx2 = tx.clone();
            tokio::spawn(async move { let _ = tx2.send(ev).await; });
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            eprintln!("[brevly-print] Pusher: print channel closed — exiting");
            break 'inner true;
        }
    }
}
```

---

### WR-05: `UPDATE` Rows-Affected Count Is Silently Discarded — Zero-Row Updates Go Unlogged

**File:** `src/print_worker.rs:92-101` and `src/print_worker.rs:137-146`

**Issue:** `conn.execute()` returns `rusqlite::Result<usize>` where the `usize` is the
number of modified rows. Both UPDATE calls use `if let Err(e) = ...` which discards
the success value entirely. If `job_id` is absent from `printed_jobs` (which should
not happen under normal flow, but could if the Pusher task's `INSERT OR IGNORE`
silently failed before the mpsc send went through), the UPDATE is a no-op with
`rows_affected = 0`. The job is acked to Noren with no database record showing it was
ever processed.

**Fix:** Log when `rows_affected == 0`:

```rust
match conn.execute(
    "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
    rusqlite::params![event.job_id],
) {
    Ok(0) => eprintln!(
        "[brevly-print] Print worker: UPDATE matched 0 rows for {} — row absent from DB",
        event.job_id
    ),
    Ok(_) => {}
    Err(e) => eprintln!(
        "[brevly-print] Print worker: SQLite update failed for {}: {e:#}",
        event.job_id
    ),
}
```

---

## Info

### IN-01: Comment Contradicts Code at `noren_client::activate()` Lines 136-140

**File:** `src/noren_client.rs:136-141`

**Issue:** The comment on line 137 states:
> "the JSON parse will fail, giving us a typed error without calling unwrap\_err"

Line 140 then calls `.unwrap_err()`. This is the contradiction that led to CR-01.
The comment was likely written for an earlier implementation that did not call
`unwrap_err`. After the CR-01 fix (adding `ActivateError::UnexpectedStatus` or
draining the body), this comment section should be updated or removed entirely.

**Fix:** Once CR-01 is remediated, delete or rewrite the `// 2xx other than 200`
comment block to describe the chosen implementation accurately.

---

### IN-02: `spike_window.rs` Is Dead Code with an Informal Removal Note

**File:** `src/lib.rs:18-19`

**Issue:** The comment reads:

```rust
// spike_window kept for reference but superseded by activation_window in Phase 2.
// Removed from main.rs startup flow.
```

The module is neither exported via `pub mod spike_window` (so external callers cannot
use it) nor deleted. The file `src/spike_window.rs` remains on disk. Rust will emit
an `unused` lint or dead-code warnings for any items inside it once the module
declaration is dropped. The "kept for reference" intent is not served well by leaving
dead code in the library crate's source tree.

**Fix:** If the file is only reference material, move it to `docs/` or delete it. If
it contains patterns worth preserving, extract comments into `PATTERNS.md` (which
already exists in `.planning/`). Remove the stale comment from `lib.rs`.

---

_Reviewed: 2026-07-16T17:52:07Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
