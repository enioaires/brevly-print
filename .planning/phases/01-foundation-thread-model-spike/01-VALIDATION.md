---
phase: 1
slug: foundation-thread-model-spike
status: planned
nyquist_compliant: true
wave_0_complete: false
created: 2026-07-15
task_ids_wired: 2026-07-15
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Derived from `01-RESEARCH.md` §Validation Architecture. Phase 1 has no v1 REQUIREMENTS
> IDs — it is a pure enabling spike; the verifiable behaviors are the four ROADMAP success
> criteria (SC-1..SC-4).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + `cargo test` |
| **Config file** | none — `Cargo.toml` `[profile.test]`; no separate framework install |
| **Quick run command** | `cargo test --target x86_64-pc-windows-msvc` |
| **Full suite command** | `cargo test --target x86_64-pc-windows-msvc -- --nocapture` |
| **Estimated runtime** | ~10 seconds (SQLite + DPAPI integration tests) |

**Note:** All automated tests run **on the Windows box / GHA Windows runner** (D-01/D-03).
The Linux planning box cannot run the msvc target, DPAPI, or wgpu — this is expected, not a gap.

---

## Sampling Rate

- **After every task commit:** Run `cargo test --target x86_64-pc-windows-msvc` (SQLite + DPAPI, <10s)
- **After every plan wave:** Run `cargo build --release` + full `cargo test`
- **Before `/gsd:verify-work`:** GHA CI `build-windows` job green (build + test)
- **Max feedback latency:** ~10 seconds (local test) / CI turnaround per push

---

## Per-Task Verification Map

Task IDs wired to the plans 2026-07-15. Format `PP-NN/Tk` = plan `01-NN`, task `k`.
Threat refs: only SC-3 (DPAPI) carries a security-relevant behavior (T-1-01, pitfall M7).
`❌ W0` = test file scaffolded in Wave 0 (plan 01-01 T3), assertions filled in the mapped plan.

| Behavior | SC | Task ID | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|----------|----|---------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| `%APPDATA%\BrevlyPrint\` created idempotently | SC-2 | 01-01/T2 | 1 | N/A (spike) | — | N/A | unit | `cargo test app_dir::tests` | ✅ (01-01/T2) | ⬜ pending |
| SQLite schema v1 (3 tables) init on first run; `user_version = 1` | SC-2 | 01-02/T1 | 2 | N/A | — | N/A | integration | `cargo test --test config_store_test` | ❌ W0 (01-01/T3) → filled 01-02/T1 | ⬜ pending |
| `config` table key/value write + read-back | SC-2 | 01-02/T1 | 2 | N/A | — | N/A | integration | `cargo test config_store_test::test_write_read` | ❌ W0 (01-01/T3) → filled 01-02/T1 | ⬜ pending |
| DPAPI encrypt → write → read → decrypt round-trip | SC-3 | 01-02/T2 | 2 | N/A | T-1-01 | Credential recoverable only in same user session (Scope::User) | integration | `cargo test credential_store_test::test_round_trip` | ❌ W0 (01-01/T3) → filled 01-02/T2 | ⬜ pending |
| Missing `credential.bin` → `CredentialError::NotFound` (no panic) | SC-3 | 01-02/T2 | 2 | N/A | T-1-01 | Missing credential degrades to re-activation, never crash | integration | `cargo test credential_store_test::test_missing_file` | ❌ W0 (01-01/T3) → filled 01-02/T2 | ⬜ pending |
| Corrupt `credential.bin` bytes → `CredentialError::Corrupt` (no panic) | SC-3 | 01-02/T2 | 2 | N/A | T-1-01 | DPAPI key loss (M7) surfaces as typed error, never crash | integration | `cargo test credential_store_test::test_corrupt_blob` | ❌ W0 (01-01/T3) → filled 01-02/T2 | ⬜ pending |
| Full v1 dependency set compiles (Windows target) | SC-4 | 01-01/T1 | 1 | N/A | — | N/A | build | `cargo build --release` | ✅ (CI, 01-01/T3) | ⬜ pending |
| egui window: text field accepts input + button triggers state change | SC-1 | 01-03/T2, T3 | 3 | N/A | — | N/A | manual | n/a — visual inspection on Windows box (01-03/T3 checkpoint) | ✅ (manual) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Scaffolded in plan **01-01 Task 3** (files compile against the store contracts; assertions
un-ignored + filled in plan 01-02). CI in the same task.

- [ ] `tests/config_store_test.rs` — SQLite init, schema v1 (3 tables), `user_version`, `config` write/read *(scaffold 01-01/T3, filled 01-02/T1)*
- [ ] `tests/credential_store_test.rs` — DPAPI round-trip, missing file, corrupt blob (`#![cfg(target_os = "windows")]`) *(scaffold 01-01/T3, filled 01-02/T2)*
- [ ] `src/app_dir.rs` inline `#[cfg(test)]` — `init_app_dir()` idempotency using a temp dir *(01-01/T2)*
- [ ] `.github/workflows/ci.yml` — Windows-runner `cargo build --release` + `cargo test` gate (`WGPU_BACKEND=dx12`) *(01-01/T3)*

---

## Manual-Only Verifications

| Behavior | SC | Task ID | Why Manual | Test Instructions |
|----------|----|---------|------------|-------------------|
| egui spike window renders; text field accepts input; button click changes a visible label/log | SC-1 | 01-03/T3 | wgpu needs a display/GPU; `winit` events need a real window session — WARP renders in CI but no display exists there | On the Windows box: `cargo run`, confirm the window opens, type into the text field, click "Aplicar", observe the label update, close the window cleanly (process exits 0) |

---

## Validation Sign-Off

- [x] All tasks have an automated verify OR a Wave 0 dependency OR a justified manual entry (SC-1 only)
- [x] Sampling continuity: no 3 consecutive tasks without automated verify (01-01: build/test/test; 01-02: test/test; 01-03: build/build/manual — the sole manual task is the last, preceded by two build verifies)
- [x] Wave 0 covers all MISSING test files above (scaffolded 01-01/T3)
- [x] No watch-mode flags in commands
- [x] Feedback latency < ~10s (local) / CI per push
- [x] `nyquist_compliant: true` set in frontmatter (task IDs wired)

**Approval:** planned — task IDs wired 2026-07-15
