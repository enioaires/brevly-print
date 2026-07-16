---
phase: 05-job-pipeline
reviewed: 2026-07-16T00:00:00Z
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
  critical: 2
  warning: 4
  info: 3
  total: 9
status: issues_found
---

# Phase 05: Code Review Report

**Reviewed:** 2026-07-16T00:00:00Z
**Depth:** standard
**Files Reviewed:** 6
**Status:** issues_found

## Summary

Reviewed the Phase 5 job pipeline: the print worker, Noren HTTP client, printer
abstraction layer, and associated tests. The Pusher client and config/credential
modules were read as supporting context to evaluate cross-module contracts.

Two blockers found: a panic-on-success path in `noren_client::activate()` and a
handle-leak in the Windows spooler when `EndPagePrinter` fails. Four warnings cover
a test that gives false confidence about the C4 ordering constraint, an unchecked
partial write from `WritePrinter`, an unvalidated job ID interpolated into HTTP URLs,
and a backpressure stall inside the Pusher inner select loop.

---

## Critical Issues

### CR-01: `unwrap_err()` Panics When Server Returns 201 with Valid JSON Body

**File:** `src/noren_client.rs:139-141`

**Issue:** The `activate()` function handles `201..=299` responses by calling
`resp.json::<ActivateResponse>().await.unwrap_err()`. The stated intent (comment on
line 136-138) is that the JSON parse will fail, producing an error. The comment even
says "without calling unwrap_err" — but the code *does* call `unwrap_err()`.

If a server returns HTTP 201 with a body that is valid `ActivateResponse` JSON, the
`.json().await` returns `Ok(response)`, and `.unwrap_err()` panics with
`"called Result::unwrap_err() on an Ok value"`. This would crash the entire process
during the activation flow.

The comment and the code contradict each other, confirming this is an unfinished or
incorrectly ported branch. The panic is not hypothetical: some REST frameworks return
`201 Created` with the same body as `200 OK`.

**Fix:** Replace the `unwrap_err()` construction with a plain `anyhow::Error` that
does not require parsing the body:

```rust
201..=299 => Err(ActivateError::Transport(
    // SAFETY: error_for_status() returns Ok for 2xx, so we cannot use it here.
    // Construct a synthetic reqwest error by reading the unexpected body.
    // Use a custom variant instead to avoid the unwrap_err() trap.
    //
    // Simplest fix: bail with a descriptive Transport-wrapped error.
    // Since ActivateError::Transport wraps reqwest::Error, and we cannot
    // construct one directly, downgrade to an anyhow error or add a new variant:
    return Err(anyhow::anyhow!("activate: unexpected 2xx status {}", resp.status()).into())
)),
```

Or, add an `Other(anyhow::Error)` variant to `ActivateError` and use it here, which
avoids the need to produce a `reqwest::Error` at all. The simplest safe fix today:

```rust
201..=299 => Err(ActivateError::InvalidSerial), // treat unexpected 2xx as invalid
// OR — if the caller needs to distinguish this:
// consume and discard the body, then return a typed anyhow error
201..=299 => {
    let _ = resp.bytes().await; // drain body to allow connection reuse
    Err(ActivateError::Transport(
        reqwest::Client::new()
            .get("http://invalid-url-that-will-never-exist-local")
            // This is still wrong. The real fix is a new ActivateError variant.
    ))
}
```

The cleanest fix is to add `ActivateError::UnexpectedStatus(u16)` and use it here.

---

### CR-02: `EndPagePrinter` Failure Leaves Zombie Spooler Document

**File:** `src/printer/spooler.rs:163-167`

**Issue:** In `submit_job()`, after `WritePrinter` succeeds and `EndPagePrinter` is
called, a `false` return from `EndPagePrinter` causes an early return without calling
`EndDocPrinter`. At this point `doc_started = true` but `EndDocPrinter` is never
called, leaving the print job document open in the Windows spooler indefinitely (a
"zombie" spooler job).

The `cleanup_on_err!` macro exists precisely for this purpose and is used correctly
on the `StartPagePrinter`, `WritePrinter`, and earlier error paths. It is missing on
the `EndPagePrinter` and `EndDocPrinter` error paths.

Note: `EndDocPrinter` failure on line 168-171 does not need cleanup (the doc sequence
is logically complete at that point), but `EndPagePrinter` failure on line 163-166
does.

```rust
// CURRENT — buggy:
if !EndPagePrinter(handle).as_bool() {
    return Err(PrinterError::PrintFailed(format!(
        "EndPagePrinterW failed for {printer_name}"
    )));
}

// FIXED:
if !EndPagePrinter(handle).as_bool() {
    // doc_started is true, page_started is true — end the doc to close the spooler job.
    let _ = EndDocPrinter(handle);
    return Err(PrinterError::PrintFailed(format!(
        "EndPagePrinterW failed for {printer_name}"
    )));
}
```

The `cleanup_on_err!` macro can also be expanded to cover this case, but since
`page_started` is always `true` at this point, a direct `EndDocPrinter` call is
clearer.

---

## Warnings

### WR-01: C4 Ordering Test Is Vulnerable to False Positives

**File:** `tests/print_worker_test.rs:139-156`

**Issue:** `update_precedes_ack_in_source` uses `src.find(...)` (first occurrence) to
locate the `UPDATE` statement and `src.rfind(...)` (last occurrence) to locate
`ack_job(`. There are TWO `UPDATE` statements in `print_worker.rs`: one at line 93
(disabled-type branch) and one at line 138 (success branch). `find()` always returns
the disabled-type branch's position (line 93).

If a future refactor accidentally inverts the ordering in the **success path only**
(e.g., moves the success-path `ack_job` to line 135 and `UPDATE` to line 150), the
test would still pass because:
- `update_idx` = position of the disabled-type UPDATE (line 93) — still first
- `ack_idx` = position of the last `ack_job(` (line 150) — still after 93

The test asserts `93 < 150`, which passes, giving false confidence that the C4
constraint holds on the success path when it does not.

**Fix:** Assert separately for each code path, or use positional anchors that
unambiguously identify the success-path block:

```rust
// Assert the SUCCESS-PATH UPDATE precedes the SUCCESS-PATH ack_job.
// Use the second (rfind equivalent for UPDATE) and last ack_job.
let success_update_idx = src
    .rfind("UPDATE printed_jobs SET status='printed'")
    .expect("success-path UPDATE not found");

let success_ack_idx = src
    .rfind("ack_job(")
    .expect("success-path ack_job not found");

assert!(
    success_update_idx < success_ack_idx,
    "C4 violated on success path: UPDATE ({success_update_idx}) must precede ack_job ({success_ack_idx})"
);
```

This uses `rfind` for both, so both point to their last (success-path) occurrence,
which correctly validates the success path.

---

### WR-02: `WritePrinter` Partial-Write Not Detected

**File:** `src/printer/spooler.rs:148-160`

**Issue:** `WritePrinter` can return `TRUE` (success) while writing fewer bytes than
requested. The current code only checks `write_ok.as_bool()` and considers the write
complete if it returns `true`. The `bytes_written` output parameter is populated but
only referenced in the error message for the failure case. A successful `TRUE` return
with `bytes_written < data_len_u32` silently produces truncated ESC/POS output —
the printer receives an incomplete command stream, which may cause garbled output,
paper not cut, or the printer hanging waiting for more bytes.

**Fix:** After the `WritePrinter` success branch, verify the count:

```rust
if !write_ok.as_bool() {
    cleanup_on_err!();
    return Err(PrinterError::PrintFailed(format!(
        "WritePrinter failed for {printer_name}: only {bytes_written}/{} bytes written",
        data.len()
    )));
}
// Guard against silent partial write (Win32 WritePrinter can succeed with fewer bytes).
if bytes_written != data_len_u32 {
    cleanup_on_err!();
    return Err(PrinterError::PrintFailed(format!(
        "WritePrinter partial write for {printer_name}: {bytes_written}/{data_len_u32} bytes"
    )));
}
```

---

### WR-03: `job_id` Interpolated Directly Into HTTP URL Without Validation

**File:** `src/noren_client.rs:244` and `src/noren_client.rs:282`

**Issue:** Both `fetch_job_bytes` and `ack_job` construct their URLs by interpolating
`job_id` directly into the path:

```rust
let url = format!("{base_url}/api/agent/jobs/{job_id}/bytes");
let url = format!("{base_url}/api/agent/jobs/{job_id}/ack");
```

`job_id` originates from the Pusher event payload, which is attacker-controlled if
the Pusher channel is compromised or a man-in-the-middle attack is possible (the
Pusher connection itself is TLS, but the channel data is not end-to-end
authenticated beyond the subscription auth). A `job_id` value of `"../admin/secret"`
would produce `GET /api/agent/jobs/../admin/secret/bytes` which some HTTP servers
normalize to `GET /api/agent/admin/secret`. A `job_id` of `"x/../../other-endpoint"`
escapes further. The agent sends this to the trusted Noren backend, so the immediate
impact is limited — but it represents a confused-deputy path if Noren's internal
routing is exploitable.

**Fix:** Validate `job_id` before interpolation:

```rust
fn validate_job_id(job_id: &str) -> anyhow::Result<()> {
    if job_id.is_empty() || job_id.contains('/') || job_id.contains('.') || job_id.contains('\\') {
        anyhow::bail!("fetch_job_bytes: invalid job_id: {job_id:?}");
    }
    Ok(())
}
```

Or use `percent_encoding` to encode the path segment before interpolation, which is
the safer approach since it handles all special characters.

---

### WR-04: `tx.send().await` Inside `select!` Can Starve the Ping Timer

**File:** `src/pusher/client.rs:328-332`

**Issue:** In the inner event loop, the `print:job` handler calls
`tx.send(event).await` inside the `ws.next()` arm of a `tokio::select!`. If the
mpsc channel buffer (32 slots, set in `main.rs:394`) is full because the print worker
is busy, `tx.send().await` blocks indefinitely inside the `select!` arm. While
blocked, the `ping_timer.tick()` arm is not polled. If the send remains blocked for
more than 30 seconds, the next successful tick would immediately fire the zombie
detection (`awaiting_pong = true` → `break 'inner true`) or miss sending the ping
entirely. Under sustained load (burst of 32+ jobs), this can trigger a spurious
reconnect or allow a truly zombie connection to go undetected.

**Fix:** Use `try_send` to avoid blocking, or spawn a separate task for the send, or
use `tokio::select!` with a timeout on the send:

```rust
// Option A: try_send and log on channel full
Ok(true) => {
    if tx.try_send(event).is_err() {
        eprintln!("[brevly-print] Pusher: print channel full — event dropped");
    }
}

// Option B: spawn and forget (preserves backpressure story externally)
Ok(true) => {
    let tx2 = tx.clone();
    tokio::spawn(async move { let _ = tx2.send(event).await; });
}
```

Option A is simpler but loses the event on overflow. Option B defers blocking outside
the select. Given the system's "no lost jobs" guarantee, the channel size should be
increased or a bounded queue with overflow-to-disk should be used in Phase 6.

---

## Info

### IN-01: Missing Test for `ack_job` HTTP 200 Happy Path

**File:** `tests/print_worker_test.rs`

**Issue:** The test suite covers `ack_job` returning `Ok(())` for 409 and `Err` for
500, but there is no test for the `200` case. The implementation treats both 200 and
409 as `Ok(())`, but the dominant success case (200) is untested. A future refactor
that accidentally changes the 200 branch would go undetected.

**Fix:** Add:

```rust
#[tokio::test]
async fn test_ack_job_200_returns_ok() {
    let base_url = spawn_stub(200, "{}").await;
    let client = reqwest::Client::new();
    let result = ack_job(&client, &base_url, "tok-test", "job-001").await;
    assert!(result.is_ok(), "200 must be Ok(()) — normal success path");
}
```

---

### IN-02: SQLite `UPDATE` Rows-Affected Never Verified

**File:** `src/print_worker.rs:92-101` and `src/print_worker.rs:137-146`

**Issue:** `conn.execute()` returns `rusqlite::Result<usize>` where the `usize` is
the number of rows modified. Both UPDATE calls discard the `Ok(n)` value with
`if let Err(e) = ...`. If the `job_id` is absent from `printed_jobs` (which should
not happen under normal flow, but could if the Pusher task's INSERT failed silently
and the mpsc send still went through), the UPDATE is a silent no-op. The job is then
acked to Noren with the database row remaining absent, creating an invisible gap in
the audit trail.

**Fix:** Log when `rows_affected == 0`:

```rust
match conn.execute(
    "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
    rusqlite::params![event.job_id],
) {
    Ok(0) => eprintln!(
        "[brevly-print] Print worker: UPDATE matched 0 rows for {} — job absent from DB",
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

### IN-03: Comment Directly Contradicts Code in `noren_client::activate()`

**File:** `src/noren_client.rs:136-141`

**Issue:** The comment on line 137 says:
> "(the JSON parse will fail, giving us a typed error **without calling unwrap_err**)"

But line 140 immediately calls `.unwrap_err()`. This contradicts the comment and
indicates the code was edited after the comment was written (or the comment was
copied from a different intended implementation). Beyond being misleading, this
contradiction is what led to the CR-01 panic risk going unnoticed.

**Fix:** Once CR-01 is fixed, update the comment to accurately describe the chosen
approach. If the branch is removed entirely, the comment goes with it.

---

_Reviewed: 2026-07-16T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
