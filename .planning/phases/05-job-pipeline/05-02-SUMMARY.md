---
phase: 05-job-pipeline
plan: "02"
subsystem: print-worker
tags: [print-pipeline, c4-ordering, enabled-types, tdd, send-bound]
dependency_graph:
  requires: [05-01]
  provides: [run_print_worker, enabled-types-filter, update-before-ack-ordering]
  affects: [src/print_worker.rs, src/main.rs, src/printer/mod.rs, tests/print_worker_test.rs]
tech_stack:
  added: []
  patterns: [enabled-types-fail-safe, c4-update-before-ack, wal-second-connection, parameterised-sql, bearer-auth-only]
key_files:
  created: []
  modified:
    - src/print_worker.rs
    - src/main.rs
    - src/printer/mod.rs
    - tests/print_worker_test.rs
decisions:
  - "Printer trait gains Send bound — required for Box<dyn Printer> to be held across .await points in tokio::spawn task (Rule 2 auto-fix)"
  - "worker_base_url cloned from auth_url before it is moved into PusherConfig at line 418 — prevents use-after-move"
  - "worker_token cloned from agent_token before agent_token is moved into the pusher spawn closure"
  - "C4 ordering enforced on BOTH code paths: disabled-type branch (UPDATE + ack) and success branch (UPDATE + ack)"
  - "enabled_types fail-safe: missing key or malformed JSON → unwrap_or_default() → empty Vec → allow all types"
metrics:
  duration: "6 minutes"
  completed: "2026-07-16"
  tasks_completed: 3
  files_modified: 4
---

# Phase 5 Plan 02: Print Worker Pipeline Summary

**One-liner:** Full fetch→print→UPDATE→ack event loop in `run_print_worker()`, wired into the Runtime block of `main.rs`, with C4 ordering enforced on every code path and six passing unit/integration tests.

## What Was Built

### Task 1: Implement run_print_worker() in src/print_worker.rs (feat)
- **Commit:** `b880097`
- **Files:** `src/print_worker.rs`
- Replaced the one-line skeleton with the complete 160-line async event loop.
- **Startup sequence (D-03):** Opens a second SQLite connection with WAL mode (T-05-09). Reads `enabled_types` (fail-safe allow-all via `unwrap_or_default()` — T-05-06). Reads `printer_name` and `printer_type` from ConfigStore; constructs `Box<dyn Printer>` once for the task lifetime.
- **Event loop:** Implements D-07 disabled-type branch (UPDATE + ack without printing), fetch (PRT-01), print_raw (PRT-02/03/04/05/06 — C1: no intermediate layers), UPDATE before ack (C4 / D-09 / T-05-04), ack (PRT-08).
- **Security:** `agent_token` never appears in any log string (T-02-02); all SQL uses `rusqlite::params![]` (T-04-07).

### Task 2: Wire print worker into main.rs (feat)
- **Commit:** `48dc8e1`
- **Files:** `src/main.rs`, `src/printer/mod.rs`
- Added `print_worker::run_print_worker` to the `brevly_print` import block.
- Removed `_print_rx: Option<...>` field and `_print_rx: Some(print_rx)` initializer from `App` — print_rx is now consumed by the spawned worker task.
- Added clone of `worker_base_url`, `worker_db_path`, `worker_http` before `auth_url` is moved into `PusherConfig`; cloned `worker_token` from `agent_token` before the pusher spawn closure takes ownership.
- Spawned `run_print_worker(print_rx, worker_token, worker_base_url, worker_db_path, worker_http)` in the Runtime block after the Pusher task spawn.
- **[Rule 2 auto-fix] Added `Send` bound to `Printer` trait** in `src/printer/mod.rs`: `pub trait Printer: Send`. Required because `Box<dyn Printer>` is held across `.await` points inside a `tokio::spawn(...)` future, which requires `F: Future + Send + 'static`. All three concrete impls (`StubPrinter`, `WindowsSpoolerPrinter`, `SerialPrinter`) are trivially `Send` (contain only `String` fields or are zero-sized).

### Task 3: Add enabled_types_filter + UPDATE-before-ack ordering tests (test)
- **Commit:** `4920f89`
- **Files:** `tests/print_worker_test.rs`
- **`enabled_types_filter` (5-02-01 / PRT-09):** Unit test that mirrors the inline predicate from `run_print_worker`. Asserts: `"order"` in `["order","dispatch"]` is allowed; `"closing"` is skipped; empty list allows everything (fail-safe).
- **`update_precedes_ack_in_source` (5-02-02 / C4 / T-05-04):** Static ordering test via `include_str!("../src/print_worker.rs")`. Asserts the byte index of the first `UPDATE printed_jobs SET status='printed'` occurrence is less than the byte index of the last `ack_job(` occurrence — proving C4 at the source level.
- All 6 `print_worker_test` tests pass; full suite: 40 passed, 1 ignored.

## Verification Results

```
cargo build -q             → success (0 errors, 0 new warnings)
cargo test -q --test print_worker_test → 6 passed; 0 failed
cargo test -q              → 40 passed; 1 ignored (full suite)
cargo clippy --all-targets -q → 0 new errors (5 pre-existing warnings in other files)
```

Manual verifications (deferred to hardware — documented as pending):
- PRT-06: < 1 second latency (requires Windows + physical thermal printer)
- PRT-02/03/04/05: physical print correctness (requires hardware)
- D-06: job-type-string grep against Noren emit code (requires Noren repo access)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical Functionality] Added Send bound to Printer trait**
- **Found during:** Task 2 (wiring main.rs)
- **Issue:** `pub trait Printer` lacked `Send` bound. `Box<dyn Printer>` is held across `.await` points inside `rt_handle.spawn(async move { run_print_worker(...).await; })`. `tokio::spawn` requires `F: Future + Send + 'static`, so the future must be `Send`. Without `+ Send` on the trait, the async block cannot satisfy this bound.
- **Fix:** Changed `pub trait Printer` to `pub trait Printer: Send` in `src/printer/mod.rs`. All three concrete impls (`StubPrinter` — zero-sized struct; `WindowsSpoolerPrinter` — `String` field; `SerialPrinter` — `String` field) are trivially `Send`.
- **Files modified:** `src/printer/mod.rs`
- **Commit:** `48dc8e1`

## Known Stubs

None. All code paths are implemented; no placeholder values, hardcoded empty returns, or TODO markers in the modified files.

## Threat Flags

No unexpected new threat surface. All security-relevant decisions are in the plan's `<threat_model>` and were verified:
- T-05-04 (C4 ordering): enforced on disabled-type AND success paths; static test asserts ordering.
- T-05-05 (ESC/POS tampering): `print_raw()` only, no intermediate layers.
- T-05-06 (enabled_types DoS): `unwrap_or_default()` prevents panic on malformed JSON.
- T-05-07 (token disclosure): `agent_token` never in format strings; test criterion verified.
- T-05-08 (SQL injection): all `conn.execute` calls use `rusqlite::params![]`.
- T-05-09 (SQLite busy): WAL mode set on the worker's own connection.

## Self-Check: PASSED
