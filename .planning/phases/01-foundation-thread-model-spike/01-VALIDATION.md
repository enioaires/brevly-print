---
phase: 1
slug: foundation-thread-model-spike
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-07-15
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

Task IDs are assigned by the planner; the rows below map each verifiable behavior to its
automated command. Threat refs: only SC-3 (DPAPI) carries a security-relevant behavior
(T-1-01, pitfall M7). `❌ W0` = test file created in Wave 0.

| Behavior | SC | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|----------|----|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| `%APPDATA%\BrevlyPrint\` created idempotently | SC-2 | 1 | N/A (spike) | — | N/A | unit | `cargo test app_dir::tests` | ❌ W0 | ⬜ pending |
| SQLite schema v1 (3 tables) init on first run; `user_version = 1` | SC-2 | 1 | N/A | — | N/A | integration | `cargo test config_store_test` | ❌ W0 | ⬜ pending |
| `config` table key/value write + read-back | SC-2 | 1 | N/A | — | N/A | integration | `cargo test config_store_test::test_write_read` | ❌ W0 | ⬜ pending |
| DPAPI encrypt → write → read → decrypt round-trip | SC-3 | 1 | N/A | T-1-01 | Credential recoverable only in same user session (Scope::User) | integration | `cargo test credential_store_test::test_round_trip` | ❌ W0 | ⬜ pending |
| Missing `credential.bin` → `CredentialError::NotFound` (no panic) | SC-3 | 1 | N/A | T-1-01 | Missing credential degrades to re-activation, never crash | integration | `cargo test credential_store_test::test_missing_file` | ❌ W0 | ⬜ pending |
| Corrupt `credential.bin` bytes → `CredentialError::Corrupt` (no panic) | SC-3 | 1 | N/A | T-1-01 | DPAPI key loss (M7) surfaces as typed error, never crash | integration | `cargo test credential_store_test::test_corrupt_blob` | ❌ W0 | ⬜ pending |
| Full v1 dependency set compiles (Windows target) | SC-4 | 1 | N/A | — | N/A | build | `cargo build --release` | ✅ (CI) | ⬜ pending |
| egui window: text field accepts input + button triggers state change | SC-1 | 1 | N/A | — | N/A | manual | n/a — visual inspection on Windows box | ✅ (manual) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/config_store_test.rs` — SQLite init, schema v1 (3 tables), `user_version`, `config` write/read
- [ ] `tests/credential_store_test.rs` — DPAPI round-trip, missing file, corrupt blob (`#[cfg(target_os = "windows")]`)
- [ ] `src/app_dir.rs` inline `#[cfg(test)]` — `init_app_dir()` idempotency using a temp dir
- [ ] `.github/workflows/ci.yml` — Windows-runner `cargo build --release` + `cargo test` gate (`WGPU_BACKEND=dx12`)

---

## Manual-Only Verifications

| Behavior | SC | Why Manual | Test Instructions |
|----------|----|------------|-------------------|
| egui spike window renders; text field accepts input; button click changes a visible label/log | SC-1 | wgpu needs a display/GPU; `winit` events need a real window session — WARP renders in CI but no display exists there | On the Windows box: `cargo run`, confirm the window opens, type into the text field, click the button, observe the label/log update, close the window cleanly (process exits 0) |

---

## Validation Sign-Off

- [ ] All tasks have an automated verify OR a Wave 0 dependency OR a justified manual entry (SC-1 only)
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING test files above
- [ ] No watch-mode flags in commands
- [ ] Feedback latency < ~10s (local) / CI per push
- [ ] `nyquist_compliant: true` set in frontmatter (after planner wires task IDs)

**Approval:** pending
