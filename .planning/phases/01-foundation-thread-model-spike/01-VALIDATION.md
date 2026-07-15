---
phase: 1
slug: foundation-thread-model-spike
status: planned
nyquist_compliant: true
wave_0_complete: false
created: 2026-07-15
task_ids_wired: 2026-07-15
replanned_cross_platform: 2026-07-15
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Derived from `01-RESEARCH.md` §Validation Architecture + the "## Cross-Platform Structure
> (Addendum 2026-07-15)". Phase 1 has no v1 REQUIREMENTS IDs — it is a pure enabling spike;
> the verifiable behaviors are the four ROADMAP success criteria (SC-1..SC-4).
>
> **REPLANNED 2026-07-15 (cross-platform):** commands now run on **Linux by default**
> (`x86_64-unknown-linux-gnu`, the primary dev/test loop). Only the **real DPAPI** path and
> the **Windows visual window** require Windows (Windows CI / owner's box). Task IDs re-wired.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + `cargo test` |
| **Config file** | none — `Cargo.toml` `[profile.test]`; no separate framework install |
| **Quick run command (default, Linux)** | `cargo test` |
| **Full suite command (Linux)** | `cargo test -- --nocapture` |
| **Windows-only subset** | `cargo test --test credential_store_test` (real DPAPI) + `cargo test -- --ignored` (wgpu adapter) — run on the Windows box / Windows CI |
| **Estimated runtime** | ~10 seconds (SQLite + credential-contract integration tests on Linux) |

**Command split (D-01/D-04 revised):**
- **Linux (default gate):** everything portable — app-dir, SQLite schema/migrations,
  config write/read, the `CredentialStore` trait + `CredentialError` contract (via
  `DevFileCredentialStore`), and the full portable `cargo build`.
- **Windows (CI + owner's box):** the **real DPAPI** round-trip/missing/corrupt
  (`tests/credential_store_test.rs`, `#![cfg(target_os = "windows")]`), the full v1 dep-set
  `cargo build --release`, and the **visual window** confirmation (DX12).
- **Ignored everywhere by default:** the wgpu adapter smoke test
  (`tests/window_smoke_test.rs`, `#[ignore]`) — no GPU/software-Vulkan in headless CI
  (addendum landmine). Run interactively with `cargo test -- --ignored`.

---

## Sampling Rate

- **After every task commit:** `cargo test` on Linux (SQLite + credential contract, <10s)
- **After every plan wave:** `cargo build` + full `cargo test` on Linux; Windows CI on push
- **Before `/gsd:verify-work`:** GHA CI matrix green — `build-linux` (build+test) AND
  `build-windows` (build --release + test incl. real DPAPI)
- **Max feedback latency:** ~10 seconds (local Linux) / CI turnaround per push

---

## Per-Task Verification Map

Task IDs re-wired 2026-07-15 for cross-platform. Format `PP-NN/Tk` = plan `01-NN`, task `k`.
Threat refs: only SC-3 (credential store) carries a security-relevant behavior (T-1-01, M7).
`❌ W0` = test file scaffolded in Wave 0 (plan 01-01 T3), assertions filled in the mapped plan.
**Platform** column: where the automated verify runs.

| Behavior | SC | Task ID | Wave | Platform | Threat Ref | Test Type | Automated Command | File Exists | Status |
|----------|----|---------|------|----------|------------|-----------|-------------------|-------------|--------|
| Portable core compiles (Linux target) | SC-4 | 01-01/T1 | 1 | Linux | — | build | `cargo build` | ✅ (01-01/T1) | ⬜ pending |
| Full v1 dep set compiles (Windows target) | SC-4 | 01-01/T3 | 1 | Windows CI | T-1-SC | build | `cargo build --release` (windows-latest) | ✅ (CI, 01-01/T3) | ⬜ pending |
| App dir created idempotently (both platforms) | SC-2 | 01-01/T2 | 1 | Linux | — | unit | `cargo test app_dir` | ✅ (01-01/T2) | ⬜ pending |
| CI matrix (ubuntu + windows) build+test | SC-4 | 01-01/T3 | 1 | CI | — | build/test | `.github/workflows/ci.yml` both jobs green | ✅ (01-01/T3) | ⬜ pending |
| SQLite schema v1 (3 tables) init; `user_version = 1` | SC-2 | 01-02/T1 | 2 | Linux | — | integration | `cargo test --test config_store_test` | ❌ W0 (01-01/T3) → filled 01-02/T1 | ⬜ pending |
| `config` table key/value write + read-back | SC-2 | 01-02/T1 | 2 | Linux | — | integration | `cargo test --test config_store_test` | ❌ W0 (01-01/T3) → filled 01-02/T1 | ⬜ pending |
| CredentialStore trait + CredentialError contract (Linux DevFile) | SC-3 | 01-02/T2 | 2 | Linux | T-1-01 | integration | `cargo test --test credential_contract_test` | ❌ W0 (01-01/T3) → filled 01-02/T2 | ⬜ pending |
| DPAPI encrypt→write→read→decrypt round-trip (real) | SC-3 | 01-02/T2 | 2 | Windows CI | T-1-01 | integration | `cargo test --test credential_store_test` (windows) | ❌ W0 (01-01/T3) → filled 01-02/T2 | ⬜ pending |
| Missing credential → `CredentialError::NotFound` (no panic) | SC-3 | 01-02/T2 | 2 | Linux + Windows | T-1-01 | integration | `cargo test --test credential_contract_test` (Linux) / `..._store_test` (Win) | ❌ W0 (01-01/T3) → filled 01-02/T2 | ⬜ pending |
| Corrupt credential → `CredentialError::Corrupt` (no panic) | SC-3 | 01-02/T2 | 2 | Windows CI | T-1-01 | integration | `cargo test --test credential_store_test::test_corrupt_blob` (windows) | ❌ W0 (01-01/T3) → filled 01-02/T2 | ⬜ pending |
| winit+egui event loop compiles; no tao/eframe | SC-1 | 01-03/T1 | 3 | Linux | — | build | `cargo build` + `grep -c 'tao\|eframe' src/main.rs == 0` | ✅ (01-03/T1) | ⬜ pending |
| egui render integration compiles; render test `#[ignore]` | SC-1 | 01-03/T2 | 3 | Linux | — | build | `cargo build` + `cargo test --test window_smoke_test -- --list` | ✅ (01-03/T2) | ⬜ pending |
| egui window: text field input + button → label; startup store round-trips | SC-1 | 01-03/T3 | 3 | Linux (then Windows) | T-1-01 | manual | n/a — visual (`cargo run`) on Linux first, confirmed on Windows | ✅ (manual) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Scaffolded in plan **01-01 Task 3** (files compile against the store contracts; assertions
un-ignored + filled in plan 01-02). CI matrix in the same task.

- [ ] `tests/config_store_test.rs` — SQLite init, schema v1 (3 tables), `user_version`, `config` write/read — **PORTABLE, runs on Linux** *(scaffold 01-01/T3, filled 01-02/T1)*
- [ ] `tests/credential_contract_test.rs` — `CredentialStore` trait + `CredentialError` contract via `DevFileCredentialStore` — **PORTABLE, runs on Linux** *(NEW in cross-platform re-plan; scaffold 01-01/T3, filled 01-02/T2)*
- [ ] `tests/credential_store_test.rs` — real DPAPI round-trip, missing file, corrupt blob — `#![cfg(target_os = "windows")]`, **Windows CI only** *(scaffold 01-01/T3, filled 01-02/T2)*
- [ ] `src/app_dir.rs` inline `#[cfg(test)]` — `init_app_dir()` idempotency — **runs on Linux** *(01-01/T2)*
- [ ] `tests/window_smoke_test.rs` — `#[ignore]` wgpu adapter check (headless-CI safe) *(01-03/T2)*
- [ ] `.github/workflows/ci.yml` — **matrix**: `ubuntu-latest` (`cargo build` + `cargo test`) + `windows-latest` (`cargo build --release` + `cargo test`, `WGPU_BACKEND=dx12`) *(01-01/T3)*

---

## Manual-Only Verifications

| Behavior | SC | Task ID | Why Manual | Test Instructions |
|----------|----|---------|------------|-------------------|
| egui spike window renders; text field accepts input; button changes a visible label; startup logs prove the store round-trips | SC-1 | 01-03/T3 | wgpu needs a display/GPU and `winit` needs a real window session — no display in headless CI; the render test is `#[ignore]`d | **Linux first** (D-04/D-07): `cargo run`; confirm startup logs (app dir created, `config` row written+read `skeleton_probe=ok`, credential round-trip OK), window opens, type into the field, click "Aplicar", the "Aplicado: …" label updates, close → exit 0. **Then Windows:** `git pull`, `cargo run`; repeat — credential round-trip now uses real DPAPI (Scope::User), window renders under DX12. |

---

## Validation Sign-Off

- [x] All tasks have an automated verify OR a Wave 0 dependency OR a justified manual entry (SC-1 visual only)
- [x] Commands run on **Linux by default**; only real-DPAPI + visual-window require Windows (clearly marked in the Platform column)
- [x] Sampling continuity: no 3 consecutive tasks without an automated verify — 01-01: build/unit-test/build+test; 01-02: integration/integration; 01-03: build/build/manual (the sole manual task is last, preceded by two build verifies)
- [x] Wave 0 covers all MISSING test files (scaffolded 01-01/T3), including the NEW Linux-provable `credential_contract_test.rs`
- [x] No watch-mode flags in commands; `#[ignore]` used for the GPU-dependent render test so headless CI stays green
- [x] Feedback latency < ~10s (local Linux) / CI per push
- [x] `nyquist_compliant: true` set in frontmatter (task IDs re-wired for cross-platform)

**Approval:** planned — task IDs re-wired for cross-platform 2026-07-15
