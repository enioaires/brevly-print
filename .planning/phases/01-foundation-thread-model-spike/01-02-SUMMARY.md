---
phase: 01-foundation-thread-model-spike
plan: "02"
subsystem: database
tags: [rusqlite, rusqlite_migration, sqlite, credential-store, dpapi, devfile, tdd, cross-platform]

dependency_graph:
  requires:
    - phase: "01-01"
      provides: "config_store.rs stub (todo!() bodies), credential_store impls (dpapi.rs, devfile.rs), Wave-0 test scaffolds with #[ignore]"
  provides:
    - "src/config_store.rs: real open_and_migrate (v1 migration, 3 tables), set (upsert), get (Ok(None) for absent)"
    - "tests/config_store_test.rs: 4 passing Linux integration tests (schema, idempotency, write/read, absent key)"
    - "tests/credential_contract_test.rs: 2 passing Linux trait contract tests via DevFileCredentialStore"
    - "tests/credential_store_test.rs: real DPAPI round-trip/missing/corrupt tests (#![cfg(windows)]) — compiles on Linux, runs on Windows CI"
  affects:
    - "01-03: app_dir + config_store ready for event loop wiring"
    - "Phase 2+: ConfigStore and CredentialStore now fully operational for activation flow"

tech_stack:
  added: []
  patterns:
    - "rusqlite_migration single multi-statement M::up: all 3 D-14 tables in one M::up -> user_version=1 (primary path; bundled SQLite handles multi-statement execute_batch)"
    - "get/set upsert via INSERT ... ON CONFLICT DO UPDATE SET value = excluded.value"
    - "get absent key via .optional() mapping QueryReturnedNoRows -> Ok(None)"
    - "#![cfg(target_os = 'windows')] at crate root gates Windows-only test files; compiles as empty on Linux"

key_files:
  created: []
  modified:
    - src/config_store.rs
    - tests/config_store_test.rs
    - tests/credential_contract_test.rs
    - tests/credential_store_test.rs

key_decisions:
  - "Pitfall 7 / OQ1 resolution: primary path taken — single multi-statement M::up works with rusqlite 0.40 bundled SQLite; user_version advances to 1 as required. No split needed."
  - "TDD flow for Task 2: credential impls were complete from 01-01/T3; removing #[ignore] went straight to GREEN. Wave-0 scaffolds were the RED state."

requirements-completed: []

duration: 18min
completed: "2026-07-15"
---

# Phase 01 Plan 02: ConfigStore Implementation + Credential Test Activation Summary

**SQLite migration v1 (3 tables, user_version=1) with get/set upsert live on Linux; credential store trait contract green via DevFileCredentialStore; real DPAPI tests wired for Windows CI.**

## Performance

- **Duration:** ~18 min
- **Started:** 2026-07-15T22:00Z
- **Completed:** 2026-07-15T22:18Z
- **Tasks:** 2 (both TDD)
- **Files modified:** 4

## Accomplishments

- `open_and_migrate` runs a single `M::up` creating `config`, `printed_jobs` (+ status index), and `retry_queue` tables; `user_version` advances to exactly 1; idempotent on re-open.
- `set` upserts via `INSERT ... ON CONFLICT DO UPDATE`; `get` returns `Ok(None)` for absent keys via `.optional()`.
- 4 config_store integration tests pass on Linux (schema v1, idempotency, write/read round-trip, absent key).
- 2 credential_contract tests pass on Linux (save->load round-trip, missing->NotFound via DevFileCredentialStore).
- Windows DPAPI tests (round-trip, missing, corrupt) compile on Linux and will run on Windows CI — T-1-01 mitigation wired.

## Task Commits

1. **Task 1 RED: failing config_store tests** - `8fdb31e` (test)
2. **Task 1 GREEN: implement ConfigStore** - `76570b0` (feat)
3. **Task 2: activate credential store tests** - `874d2bc` (feat)

## Files Created/Modified

- `src/config_store.rs` — replaced todo!() stubs with real migration + get/set implementation
- `tests/config_store_test.rs` — replaced #[ignore] scaffold with 4 real assertions (schema, idempotency, write/read, absent key)
- `tests/credential_contract_test.rs` — removed #[ignore]; 2 trait contract tests now active on Linux
- `tests/credential_store_test.rs` — removed #[ignore] from real DPAPI tests; #![cfg(target_os="windows")] gates them to Windows CI

## Decisions Made

**Pitfall 7 / OQ1 resolution (primary path):** A single multi-statement `M::up` with all 3 D-14 tables was tried first. It worked — bundled SQLite (via `rusqlite` "bundled" feature) handles `execute_batch` on a multi-statement string correctly. `user_version` advances to 1 as required. No split needed. If future Windows CI reveals a different behavior, the fallback is to split into 3 separate `M::up` calls (user_version would then be 3; the test assertion would need updating).

**Task 2 TDD note:** The credential store implementations (`dpapi.rs`, `devfile.rs`) were fully written in 01-01/T3. The `#[ignore]` scaffolds from 01-01 represented the "RED state" for this plan. Removing `#[ignore]` went straight to GREEN — no implementation work was needed in this plan for the credential impls. This is intentional: 01-01 delivered the impls early (per the 01-01 SUMMARY), and this plan activates the tests.

## Deviations from Plan

None — plan executed exactly as written. The credential impl being already complete from 01-01 was documented in the 01-01 SUMMARY as an intentional early delivery; 01-02/T2 activating the tests is the expected completion step.

## Threat Flags

None — no new network endpoints, auth paths, file access patterns, or schema changes beyond those specified in the plan's threat model (T-1-01 through T-1-03).

## Known Stubs

None — all stubs from 01-01 (open_and_migrate, set, get) are now implemented. The `main.rs` placeholder remains but was out of scope for this plan.

## Success Criteria Met

- [x] SC-2 proven on Linux: SQLite schema v1 (3 tables, user_version=1) + config write/read (4 tests pass)
- [x] SC-3 (Linux half) proven: CredentialStore trait + CredentialError contract via DevFileCredentialStore (2 tests pass)
- [x] SC-3 (Windows half) wired: real DPAPI round-trip/corrupt tests compiled, #![cfg(windows)] gated for Windows CI (T-1-01)
- [x] No #[ignore] remains in config_store_test.rs or credential_contract_test.rs
- [x] src/config_store.rs contains Migrations with 3 tables and to_latest

## Self-Check: PASSED

Files verified:
- src/config_store.rs: FOUND
- tests/config_store_test.rs: FOUND
- tests/credential_contract_test.rs: FOUND
- tests/credential_store_test.rs: FOUND

Commits verified:
- 8fdb31e: test(01-02): add failing config_store integration tests (RED)
- 76570b0: feat(01-02): implement ConfigStore - migration v1 (3 tables), get/set (GREEN)
- 874d2bc: feat(01-02): activate credential store tests - Linux contract green, Windows DPAPI wired
