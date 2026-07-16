---
phase: 07-auto-update-distribution-polish
plan: 01
subsystem: update
tags: [update, integrity, sha256, semver, http-client, linux-provable, tdd]
requirements: [DIST-02, DIST-03]

dependency_graph:
  requires: []
  provides:
    - src/update/verify.rs::verify_sha256
    - src/update/check.rs::check_for_update
    - src/update/mod.rs::try_check_and_stage
    - src/noren_client.rs::check_version
  affects:
    - src/lib.rs (pub mod update added)
    - src/noren_client.rs (VersionResponse + check_version added)
    - Cargo.toml (sha2, semver added to portable [dependencies])

tech_stack:
  added:
    - sha2 = "0.11.0" (portable [dependencies] — SHA-256 via RustCrypto)
    - semver = "1.0.28" (portable [dependencies] — version comparison)
  patterns:
    - health_state.rs pattern: pure enum + pure fn + in-module #[cfg(test)] block
    - tray_runtime.rs pattern: file-level #![cfg(windows)] for apply.rs
    - noren_client.rs self-analog: fetch_pending_jobs bearer-auth GET shape cloned for check_version
    - noren_client_test.rs pattern: spawn_stub TCP mock copied verbatim for update tests

key_files:
  created:
    - src/update/verify.rs
    - src/update/check.rs
    - src/update/mod.rs
    - src/update/apply.rs
    - tests/update_task_test.rs
  modified:
    - src/lib.rs (pub mod update)
    - src/noren_client.rs (VersionResponse, check_version)
    - Cargo.toml (sha2, semver)
    - Cargo.lock (updated)

decisions:
  - sha2 and semver added to portable [dependencies] (not Windows-only) because
    src/update/ is portable code that must compile on Linux
  - run_update_check_loop NOT placed in lib; deferred to main.rs (plan 02) because it
    requires crate::UserEvent which lives in the binary crate — documented in mod.rs
  - spawn_bytes_stub helper added in addition to spawn_stub to serve artifact bytes for SC-2 test

metrics:
  duration: "4m 41s"
  completed: "2026-07-16"
  tasks_completed: 3
  files_changed: 9
---

# Phase 07 Plan 01: Update Core — Linux-Provable Integrity + Decision Logic

SHA-256 integrity gate (`verify_sha256`) and semver decision logic (`check_for_update`) as pure, Linux-tested functions; `check_version` bearer-authed HTTP GET; `try_check_and_stage` orchestration with SC-2 abort proven by integration test.

## What Was Built

### Task 1: Pure integrity + decision logic (verify.rs, check.rs)

`src/update/verify.rs` — `verify_sha256(bytes: &[u8], expected_hex: &str) -> anyhow::Result<()>`:
- Computes SHA-256 via `sha2::Sha256::digest` (RustCrypto, no `hex` crate)
- Case-insensitive comparison via `eq_ignore_ascii_case` (handles uppercase hex from CI scripts)
- 6 unit tests: match, mismatch, uppercase, wrong-length, empty-match, empty-mismatch

`src/update/check.rs` — `check_for_update(current, remote) -> UpdateDecision`:
- `UpdateDecision` enum: `UpToDate | UpdateAvailable | Err(String)`
- Uses `semver::Version::parse` on both strings; returns `Err` on either failure (never panics)
- 5 unit tests: newer, older, equal, malformed-remote, malformed-current

Neither file contains `#[cfg(windows)]`, `windows`, or `velopack`.

### Task 2: Orchestration seam + check_version + apply.rs skeleton

`src/noren_client.rs` additions:
- `VersionResponse { version, download_url, sha256 }` with `#[serde(rename_all = "camelCase")]`
- `check_version(client, base_url, agent_token) -> anyhow::Result<VersionResponse>` — bearer-authed GET to `/api/agent/version`; mirrors `fetch_pending_jobs` shape exactly; token only via `.bearer_auth()` (T-02-02)

`src/update/apply.rs`:
- File-level `#![cfg(windows)]` — compiles to nothing on Linux
- `stage_update(feed_base_url)` skeleton using Velopack `UpdateManager::new + check_for_updates + download_updates + wait_exit_then_apply_updates`
- SPIKE comments for OQ-1 (staging persistence timing) and A2 (UpdateInfo field name)

`src/update/mod.rs` — `try_check_and_stage(http, base_url, agent_token) -> anyhow::Result<bool>`:
- Calls `check_version` → `check_for_update` → download bytes → `verify_sha256` → (Windows) `apply::stage_update`
- `verify_sha256` is called BEFORE any `#[cfg(windows)]` staging call (ordering enforces SC-2)
- On mismatch: `eprintln!` + `return Ok(false)` — no staging, no owner-facing error (D-02)
- Module doc comment explains why `run_update_check_loop` belongs in `main.rs` (plan 02)

`src/lib.rs`: `pub mod update;` added.

### Task 3: SC-2 abort + HTTP-mock integration tests

`tests/update_task_test.rs` — 6 tests:
1. `check_version_200_parses_camel_case` — stub 200, JSON with camelCase keys → VersionResponse fields correct
2. `check_version_500_returns_err` — stub 500 → Err, no panic
3. `sc2_mismatch_aborts_without_staging` — version stub returns newer version + artifact URL; artifact bytes stub serves real bytes; sha256 in version response is all-zeros (wrong); asserts `Ok(false)` (dual check: NOT `Ok(true)`)
4. `try_check_and_stage_bad_json_returns_err` — malformed JSON → Err, no panic
5. `pure_check_for_update_newer_returns_available` — smoke for UpdateDecision
6. `pure_verify_sha256_match_returns_ok` — smoke for verify_sha256

## Verification Results

```
cargo build --lib          → OK (apply.rs excluded via #![cfg(windows)])
cargo test --lib update    → 11/11 passed
  update::check::tests::*  — 5 tests (newer/older/equal/malformed-remote/malformed-current)
  update::verify::tests::* — 6 tests (match/mismatch/uppercase/wrong-length/empty-match/empty-mismatch)
cargo test --test update_task_test → 6/6 passed
  check_version_200_parses_camel_case
  check_version_500_returns_err
  sc2_mismatch_aborts_without_staging
  try_check_and_stage_bad_json_returns_err
  pure_check_for_update_newer_returns_available
  pure_verify_sha256_match_returns_ok
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] sha2 and semver not in portable [dependencies]**
- **Found during:** Task 1 setup (pre-compilation check)
- **Issue:** Plan stated "sha2 = '0.11.0' and semver = '1.0.28' are ALREADY in Cargo.toml" but they were only in Cargo.lock as transitive dependencies of `velopack` which is in `[target.'cfg(windows)'.dependencies]`. Portable modules in `src/update/` use these crates directly; they would fail to compile on Linux without an explicit entry in `[dependencies]`.
- **Fix:** Added `sha2 = "0.11.0"` and `semver = "1.0.28"` to the portable `[dependencies]` section with explanatory comments.
- **Files modified:** `Cargo.toml`, `Cargo.lock`
- **Commit:** b2f8f09

**2. [Rule 2 - Missing critical functionality] spawn_bytes_stub helper for SC-2 test**
- **Found during:** Task 3 design
- **Issue:** The SC-2 test must drive `try_check_and_stage` with a version stub pointing to a real artifact URL, but `spawn_stub` only serves JSON bodies. The artifact download is a raw-bytes response — a second stub with a different response format was required.
- **Fix:** Added `spawn_bytes_stub(bytes: Vec<u8>) -> String` alongside the copied `spawn_stub`. The stub serves `application/octet-stream` with the given bytes.
- **Files modified:** `tests/update_task_test.rs`
- **Commit:** 3ce1c86

## Known Stubs

- `src/update/apply.rs` — entire file is a skeleton for plan 02. `stage_update()` will work on Windows once the OQ-1 spike validates Velopack `UpdateInfo` field names and staging persistence. No user-visible stub pattern; the stub is entirely Windows-gated and never reached from tests.

## Threat Surface Scan

All security-relevant surface introduced in this plan is covered by the plan's own `<threat_model>`:
- `T-7-V6`: `verify_sha256` before any staging — ordering enforced, tested by SC-2 integration test
- `T-7-V5`: `check_for_update` + `verify_sha256` never panic on malformed input — tested
- `T-7-mismatch`: SC-2 `Ok(false)` abort — proven by integration test dual-assertion
- `T-7-DoS`: `try_check_and_stage` returns Err on HTTP failure — tested
- `T-7-V2`: `agent_token` via `.bearer_auth()` only — grep confirms no eprintln!/format!/bail! leaks

No new trust boundary surface beyond what the threat model covers.

## Commits

| Task | Commit | Description |
|------|--------|-------------|
| 1    | b2f8f09 | feat(07-01): pure verify_sha256 + check_for_update with unit tests |
| 2    | b3eff46 | feat(07-01): orchestration seam, apply.rs skeleton, check_version |
| 3    | 3ce1c86 | test(07-01): SC-2 abort + HTTP-mock integration tests |

## Self-Check: PASSED

Files verified to exist:
- src/update/verify.rs: FOUND
- src/update/check.rs: FOUND
- src/update/mod.rs: FOUND
- src/update/apply.rs: FOUND
- src/noren_client.rs (check_version): FOUND
- tests/update_task_test.rs: FOUND

Commits verified:
- b2f8f09: FOUND
- b3eff46: FOUND
- 3ce1c86: FOUND

Tests: 11/11 lib + 6/6 integration = 17/17 total green on Linux.
