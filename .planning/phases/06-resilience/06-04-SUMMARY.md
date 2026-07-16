---
phase: 06-resilience
plan: "04"
subsystem: noren_client + pusher_client
tags: [resilience, rpc, websocket, security, offline-recovery]
dependency_graph:
  requires: ["06-01"]
  provides: ["RES-03"]
  affects: ["src/noren_client.rs", "src/pusher/client.rs", "tests/pending_jobs_test.rs"]
tech_stack:
  added: []
  patterns:
    - "fetch_pending_jobs follows fetch_job_bytes bearer_auth + status match pattern"
    - "PendingJobsResponse local wrapper struct (same as BytesResponse / PusherAuthResponse)"
    - "WR-04 try_send + tokio::spawn-on-Full for non-blocking forwarding"
    - "CR-02 validate_job_id guard before insert_print_job in pending pull loop"
key_files:
  created: []
  modified:
    - src/noren_client.rs
    - src/pusher/client.rs
    - tests/pending_jobs_test.rs
decisions:
  - "Used per-field serde(rename) on PendingJob instead of blanket rename_all=camelCase — the wire key is 'type' (Rust keyword), not 'jobType'"
  - "Pending pull inserted BEFORE inner select! loop (not inside it) — avoids blocking ping timer (T-06-09)"
  - "Fetch failure is non-fatal: logs and falls through to event loop; WebSocket never torn down (D-08)"
metrics:
  duration: "226s (~4m)"
  completed: "2026-07-16"
  tasks_completed: 2
  tasks_total: 2
  files_modified: 3
---

# Phase 06 Plan 04: Offline Recovery (RES-03) Summary

RES-03 internet-outage recovery: pending job pull on every Pusher reconnect with CR-02 job_id validation, C3 dedup, and WR-04 non-blocking forwarding.

## Tasks Completed

| # | Task | Commit | Files |
|---|------|--------|-------|
| 1 | Add fetch_pending_jobs() + PendingJob; make validate_job_id pub(crate) | 2e5adbd | src/noren_client.rs, tests/pending_jobs_test.rs |
| 2 | Wire pending pull into run_pusher_loop after subscription_succeeded | 6379bcc | src/pusher/client.rs |

## What Was Built

**Task 1 — noren_client.rs:**

- `pub struct PendingJob` with `#[serde(rename = "jobId")]` and `#[serde(rename = "type")]` — per-field rename instead of blanket `rename_all` because the wire key is the Rust keyword `type`, not `jobType`.
- Private `PendingJobsResponse { jobs: Vec<PendingJob> }` wrapper struct (defined locally inside `fetch_pending_jobs`, mirroring `BytesResponse` / `PusherAuthResponse` pattern).
- `pub async fn fetch_pending_jobs(client, base_url, agent_token) -> anyhow::Result<Vec<PendingJob>>` — GET `/api/agent/jobs/pending` with bearer auth, parses `{"jobs":[...]}` wrapper, returns `Err` on non-200.
- `fn validate_job_id` promoted to `pub(crate) fn validate_job_id` so `pusher/client.rs` can call it directly.
- Wave-0 `#[ignore]` attributes removed from `pending_jobs_test.rs`; all assertions activated and passing.

**Task 2 — pusher/client.rs:**

- Added `fetch_pending_jobs` and `validate_job_id` to the `noren_client` destructured import.
- Inserted the RES-03 pending pull block immediately after `send_health(HealthState::Connected)` + `attempt = 0`, before the inner `select!` loop (Step 7):
  - CR-02 guard: `validate_job_id(&job.job_id)` before `insert_print_job` — rejects `/`, `.`, `\`, `?`, `#`, `%`, `\0`.
  - `insert_print_job` -> `Ok(true)` -> `tx.try_send` with spawn-on-Full (WR-04).
  - `Ok(false)` -> C3 dedup no-op.
  - `Err(e)` from fetch -> `eprintln!` only, falls through to inner event loop (D-08 invariant).

## Verification Results

```
cargo test fetch_pending_jobs   -> 3 passed
cargo test --test pending_jobs_test -> 4 passed
cargo build -> Finished (2.95s)
```

Existing tests: `insert_print_job_returns_false_on_duplicate` and all pusher/client.rs unit tests still GREEN.

Note: `config_store_test.rs` failures (`user_version=2`, `status='printing'`) are pre-existing RED stubs from plan 06-01 (migration v2 not yet applied). These are the responsibility of plan 06-02/06-03.

## Deviations from Plan

None — plan executed exactly as written.

The PATTERNS.md snippet for `PendingJob` used `rename_all = "camelCase"` with field `job_type` (which would map to `jobType`), but the plan explicitly corrected this: use per-field `rename` because the wire key is `type`. Implemented per plan (D-09 contract correction).

## Known Stubs

None — all code paths are fully implemented.

## Threat Flags

All threats from the plan's threat model are addressed:

| Threat | Mitigation Applied |
|--------|--------------------|
| T-06-07 path traversal | `validate_job_id` called on every `job.job_id` before `insert_print_job` |
| T-06-08 token disclosure | `agent_token` only via `.bearer_auth()`, never in `eprintln!` |
| T-06-09 pending pull blocks ping timer | Pull runs before inner `select!` loop; forwarding uses `try_send` (non-blocking) |

No new trust boundaries introduced beyond those in the plan's threat model.

## Self-Check: PASSED

- [x] `src/noren_client.rs` contains `pub async fn fetch_pending_jobs(`
- [x] `src/noren_client.rs` contains `pub(crate) fn validate_job_id(`
- [x] `PendingJob` uses `#[serde(rename = "jobId")]` and `#[serde(rename = "type")]`
- [x] `PendingJobsResponse { jobs: Vec<PendingJob> }` wrapper present
- [x] `src/pusher/client.rs` calls `fetch_pending_jobs(&http, &config.auth_url, &agent_token)`
- [x] `validate_job_id` called before `insert_print_job` in pending pull block
- [x] WR-04 `try_send` + `tokio::spawn` on Full
- [x] Fetch error only logs — no outer loop break
- [x] Commits 2e5adbd and 6379bcc exist in git log
