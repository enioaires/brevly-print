---
phase: 05-job-pipeline
plan: "01"
subsystem: noren-client
tags: [http-client, base64, contract-tests, tdd]
dependency_graph:
  requires: []
  provides: [fetch_job_bytes, ack_job, base64-dep, print_worker-skeleton, print_worker-tests]
  affects: [src/noren_client.rs, src/print_worker.rs, src/lib.rs, tests/print_worker_test.rs, Cargo.toml]
tech_stack:
  added: [base64 = "0.22" (direct dep, was transitive 0.22.1)]
  patterns: [bearer-auth-only, base64-Engine-API, mock-TCP-stub-tests, anyhow-context-chain, idempotent-409-ack]
key_files:
  created:
    - src/print_worker.rs
    - tests/print_worker_test.rs
  modified:
    - src/noren_client.rs
    - src/lib.rs
    - Cargo.toml
    - Cargo.lock
decisions:
  - "409 on ack_job maps to Ok(()) — idempotent by design (C4/D-04): post-crash ack repeat is normal"
  - "base64 Engine API (STANDARD.decode) used — not the deprecated 0.21 free functions"
  - "agent_token only via .bearer_auth() — never in format!/eprintln!/context() strings (T-05-01/T-02-02)"
metrics:
  duration: "3 minutes"
  completed: "2026-07-16"
  tasks_completed: 2
  files_modified: 6
---

# Phase 5 Plan 01: HTTP Client Primitives (fetch_job_bytes + ack_job) Summary

**One-liner:** Authenticated HTTP fetch (GET + base64 decode) and idempotent ack (POST, 409=Ok) for the Noren print job pipeline, with four passing mock-TCP contract tests.

## What Was Built

### Task 1: base64 dep + print_worker skeleton + Wave-0 test scaffold (chore)
- **Commit:** `9070c9a`
- **Files:** `Cargo.toml`, `src/lib.rs`, `src/print_worker.rs`, `tests/print_worker_test.rs`
- Added `base64 = "0.22"` as an explicit direct dependency (was transitive 0.22.1 in Cargo.lock).
- Created `src/print_worker.rs` as a minimal skeleton (doc comment only) so the crate compiles for testing.
- Added `pub mod print_worker;` to `src/lib.rs` after `pub mod pusher;`.
- Created `tests/print_worker_test.rs` with the `spawn_stub` helper (copied verbatim from `tests/noren_client_test.rs`) and four `#[tokio::test]` contract tests for `fetch_job_bytes` and `ack_job`. Tests were RED (compile error) until Task 2.

### Task 2: Implement fetch_job_bytes() and ack_job() (TDD — GREEN) (feat)
- **Commit:** `693cf02`
- **Files:** `src/noren_client.rs`, `Cargo.lock`
- Added `fetch_job_bytes()` at the bottom of `src/noren_client.rs` mirroring `pusher_auth()` structure:
  - `GET /api/agent/jobs/{job_id}/bytes`
  - Local `BytesResponse { bytes: String }` struct (same pattern as `PusherAuthResponse`)
  - 200 → `base64::engine::general_purpose::STANDARD.decode(&body.bytes)` → `Vec<u8>`
  - Any other status → `anyhow::bail!("fetch_job_bytes: unexpected status {status}")`
- Added `ack_job()`:
  - `POST /api/agent/jobs/{job_id}/ack` (no request body)
  - `200 | 409 => Ok(())` — 409 is idempotent (C4/D-04)
  - Any other status → `anyhow::bail!("ack_job: unexpected status {status}")`
- Both functions use `.bearer_auth(agent_token)` exclusively — token never in any error string.
- All 4 Wave-0 contract tests pass.

## Verification Results

```
cargo build -q        → success (0 errors, 0 new warnings)
cargo test -q --test print_worker_test → 4 passed; 0 failed
```

## Deviations from Plan

None — plan executed exactly as written.

## Known Stubs

None. `src/print_worker.rs` is intentionally a skeleton: it exists to allow the crate to compile and tests to import `brevly_print::noren_client`. Plan 02 implements `run_print_worker()` — the stub does not block this plan's goal (the goal is the HTTP primitives, not the print worker loop).

## Threat Flags

No unexpected new threat surface. The two new functions (`fetch_job_bytes`, `ack_job`) are explicitly modeled in the plan's `<threat_model>`:
- T-05-01: agent_token never in logs — mitigated via `.bearer_auth()` only
- T-05-02: base64 decode of untrusted input — mitigated via `STANDARD.decode()` returning `Result`
- T-05-03: 409 retry storm — mitigated by mapping 409 to `Ok(())`

## Self-Check: PASSED
