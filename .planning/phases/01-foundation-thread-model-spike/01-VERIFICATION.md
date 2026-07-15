---
phase: 01-foundation-thread-model-spike
verified: 2026-07-15T23:05:00Z
status: passed
score: 4/4 must-haves verified
overrides_applied: 0
re_verification:
  # No previous VERIFICATION.md existed — this is the initial verification.
---

# Phase 1: Foundation + Thread Model Spike Verification Report

**Phase Goal:** Prove the `winit 0.30` + raw `egui` (`egui-winit` + `egui-wgpu`) event-loop integration and initialize all persistence infrastructure on a cross-platform-buildable base (portable core builds+tests on Linux AND Windows; product v1 stays Windows-only) so every subsequent phase builds on a validated base.
**Verified:** 2026-07-15T23:05:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| SC-1 | `winit 0.30` `ApplicationHandler` event loop drives a raw egui window (`egui-winit` + `egui-wgpu`) with NO separate Win32 loop, no tao, no eframe — proven on Linux, confirmed on Windows (DX12) | ✓ VERIFIED | `impl ApplicationHandler<UserEvent> for App` (src/main.rs:43); `EventLoop::with_user_event()...run_app()` (src/main.rs:153,161); egui input forwarded via `state.on_window_event` (src/spike_window.rs:69); raw egui-wgpu `Renderer` render pass (src/spike_window.rs:55-128). `grep -cE 'tao\|eframe'` = 0 in both Cargo.toml and src/. Owner human-verify checkpoint APPROVED on Linux (Rgba8UnormSrgb/Vulkan) AND confirmed on Windows (DX12, real DPAPI) per 01-03-SUMMARY.md:38-50. |
| SC-2 | SQLite `state.db` initializes with tables `config`, `printed_jobs`, `retry_queue` on first run at per-platform app dir (dirs crate); verified by `cargo test` | ✓ VERIFIED | `MIGRATIONS` single `M::up` creates all 3 tables + status index (src/config_store.rs:30-61); `to_latest` wired (src/config_store.rs:75); app dir via `dirs::data_dir().join("BrevlyPrint")` + `create_dir_all` (src/app_dir.rs:20-27). Test `test_schema_v1_and_user_version` asserts `user_version=1` and all 3 tables exist — PASSED. 4/4 config_store tests + 2/2 app_dir tests green on Linux. |
| SC-3 | Credentials round-trip through `CredentialStore` trait; missing/corrupt → typed `CredentialError` (never panics) on both impls. Windows uses DPAPI `Scope::User`; Linux dev impl exists so contract tests pass on Linux | ✓ VERIFIED | `trait CredentialStore { save/load -> Result<_,CredentialError> }` (src/credential_store/mod.rs:23-32); `CredentialError` enum NotFound/Corrupt/Io via thiserror, no panics (src/credential_store/error.rs:12-30); DPAPI impl uses `encrypt_data/decrypt_data(_, Scope::User, _)` gated `#![cfg(windows)]` (src/credential_store/dpapi.rs:7,35-48); DevFile impl gated `#![cfg(not(windows))]` (src/credential_store/devfile.rs:10). Linux contract tests (round-trip + NotFound) PASSED (2/2). Windows DPAPI tests (round-trip/missing/corrupt) `#![cfg(target_os="windows")]` compile as empty on Linux, run on Windows CI + confirmed on owner box (01-03-SUMMARY.md:49). |
| SC-4 | Cargo compiles portable core on `x86_64-unknown-linux-gnu` AND full v1 dep set on `x86_64-pc-windows-msvc` (Windows-only crates under `[target.'cfg(windows)'.dependencies]`) | ✓ VERIFIED | Local `cargo build` exits 0 on Linux. `[target.'cfg(windows)'.dependencies]` (Cargo.toml:52) contains windows 0.62, windows-dpapi 0.2, tray-icon 0.24, printers 2, serialport 4.9, auto-launch 0.6, velopack 1, tauri-winrt-notification 0.8 — all 8 required Windows crates present and correctly gated. Windows half proven by CI job `build-windows` (`cargo build --release --target x86_64-pc-windows-msvc`, .github/workflows/ci.yml:69-70) + owner confirmation on Windows box. |

**Score:** 4/4 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `Cargo.toml` | Target-gated manifest (portable + `[target.'cfg(windows)'.dependencies]`) | ✓ VERIFIED | Line 16 portable `[dependencies]`; line 52 Windows-only block with all 8 crates. wgpu features include dx12+vulkan+metal+gles (line 23) for cross-platform. |
| `src/lib.rs` | Lib root re-exporting app_dir, config_store, credential_store, spike_window | ✓ VERIFIED | 12 lines, `pub mod` for all 4 modules + `pub use init_app_dir`. |
| `src/app_dir.rs` | `init_app_dir()` cross-platform idempotent | ✓ VERIFIED | `create_dir_all` (line 27); 2 inline tests pass. |
| `src/config_store.rs` | rusqlite_migration v1 (3 tables) + get/set | ✓ VERIFIED | `Migrations`, `M::up`, `to_latest`, upsert `set`, `.optional()` `get`. No `todo!()` remains. |
| `src/credential_store/mod.rs` | Trait + cfg-gated impl selection | ✓ VERIFIED | `trait CredentialStore` + cfg-gated `credential_store()` factory. |
| `src/credential_store/dpapi.rs` | Windows DPAPI `Scope::User` impl | ✓ VERIFIED | Real `encrypt_data`/`decrypt_data`; existence check before decrypt; `#![cfg(windows)]`. |
| `src/credential_store/devfile.rs` | Linux dev impl (non-secure) | ✓ VERIFIED | Plaintext file impl, `#![cfg(not(windows))]`, clear "DEV/TEST ONLY — NOT SECURE" doc. |
| `src/main.rs` | winit ApplicationHandler + startup store wiring | ✓ VERIFIED | Full startup path (velopack gated → app_dir → migrate → config set/get → credential save/load) then `run_app`. |
| `src/spike_window.rs` | egui-winit + egui-wgpu render + spike UI | ✓ VERIFIED | Full Instance→adapter→device→surface→egui-wgpu pipeline; text field + "Aplicar" button + "Aplicado:" label; surface format from caps (not hardcoded). |
| `tests/config_store_test.rs` | SQLite schema + write/read integration tests | ✓ VERIFIED | 4 tests, `user_version` asserted; all PASS on Linux. |
| `tests/credential_contract_test.rs` | Linux trait + error contract test | ✓ VERIFIED | 2 tests (round-trip + NotFound); PASS on Linux. |
| `tests/credential_store_test.rs` | Windows DPAPI round-trip/missing/corrupt | ✓ VERIFIED | `#![cfg(target_os="windows")]`; 3 tests; compiles empty on Linux, runs on Windows CI. |
| `tests/window_smoke_test.rs` | headless-safe `#[ignore]` wgpu adapter check | ✓ VERIFIED | `#[ignore]`d; correctly skipped in headless `cargo test`. |
| `.github/workflows/ci.yml` | ubuntu + windows CI matrix (build + test) | ✓ VERIFIED | `build-linux` (apt deps + build + test) and `build-windows` (`--target x86_64-pc-windows-msvc` build --release + test, WGPU_BACKEND=dx12, RUST_TEST_THREADS=1). |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| src/lib.rs | src/app_dir.rs | `pub mod app_dir` | ✓ WIRED | lib.rs:6 |
| src/credential_store/mod.rs | dpapi/devfile | `#[cfg(windows)]` / `#[cfg(not(windows))]` mod | ✓ WIRED | mod.rs:10-14, 38-45 |
| src/config_store.rs | rusqlite_migration | `MIGRATIONS.to_latest(&mut conn)` | ✓ WIRED | config_store.rs:74-75 |
| tests/config_store_test.rs | brevly_print::config_store | `open_and_migrate` on temp db | ✓ WIRED | test file uses tempdir + open_and_migrate |
| src/main.rs | config_store::open_and_migrate | AppState startup after init_app_dir | ✓ WIRED | main.rs:131 |
| src/main.rs | credential_store::credential_store | cfg-selected impl round-trip at startup | ✓ WIRED | main.rs:144-149 |
| src/spike_window.rs | egui_winit::State::on_window_event | window_event forwards input to egui | ✓ WIRED | spike_window.rs:69, called from main.rs:67 |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| spike UI label | `applied` | copied from `input` on button click (spike_window.rs:332-335) | Yes — driven by live text-edit + button | ✓ FLOWING |
| config probe | `probe_val` | `config_store::get` after `set("skeleton_probe","ok")` (main.rs:136-141) | Yes — real SQLite round-trip, asserted `Some("ok")` | ✓ FLOWING |
| credential probe | `loaded` | `cred.load()` after `cred.save(b"skeleton-dummy")` (main.rs:146-149) | Yes — real store round-trip, asserted equal | ✓ FLOWING |

Note: Phase 1 credential value is an intentional hardcoded dummy (`b"skeleton-dummy"`); real tokens arrive in Phase 2. This is a spike probe, not user-facing dynamic data, so it is correct-by-design for SC-1.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Portable core builds on Linux | `cargo build` | exit 0 | ✓ PASS |
| Full Linux test suite green | `cargo test` | 8 passed, 1 ignored, 0 failed (7 suites) | ✓ PASS |
| No tao/eframe in manifest or src | `grep -cE 'tao\|eframe'` | 0 in Cargo.toml, 0 in src/ | ✓ PASS |
| wgpu smoke test gated | `cargo test` | `test_wgpu_adapter_available ... ignored` | ✓ PASS |
| Windows-gated DPAPI test empty on Linux | test suite | credential_store_test = 0 tests on Linux | ✓ PASS |

### Probe Execution

No conventional `scripts/*/tests/probe-*.sh` probes and no PLAN-declared probes for this phase. Verification uses `cargo test` (native Rust test harness) as the runnable check — executed above. Section N/A.

### Requirements Coverage

Phase 1 is a pure enabling spike with no v1 REQUIREMENTS IDs (ROADMAP.md:28 — "Requirements: (none)"). No requirement rows to cover; no orphans. The four SCs are the verifiable contract and all pass.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| Cargo.toml | 70 | `# TODO Phase 5: move serialport to portable` | ℹ️ Info | Documented forward-plan comment for a deliberately Windows-gated dep; not a Phase 1 stub. No debt-marker gate violation (TODO, not TBD/FIXME/XXX). |
| src/main.rs | 24 | `UserEvent placeholder (tray wiring is Phase 3)` | ℹ️ Info | Empty enum comment for a Phase 3 extension point; the event loop is fully functional without it. Not a stub blocking Phase 1's goal. |

No blocker debt markers (TBD/FIXME/XXX) present. No `todo!()`/`unimplemented!()` remain — the 01-01 config_store stubs were filled in 01-02 as claimed. No orphaned or hollow artifacts.

### Human Verification Required

None outstanding. The single manual checkpoint for this phase (SC-1 visual window render + interaction, including the Windows/DX12/real-DPAPI confirmation) was already executed and APPROVED by the owner on Linux AND confirmed on Windows during Plan 01-03 execution (01-03-SUMMARY.md:38-50). That checkpoint is the intended sink for the deferred human-verify item; no new items were identified. Windows-only proofs (DX12 render, real DPAPI round-trip, windows-msvc full-dep compile) rest on that owner confirmation plus the `build-windows` CI job — both present and accepted.

### Gaps Summary

None. All 4 ROADMAP success criteria are verified in the actual codebase:

- SC-1 is proven by a real `ApplicationHandler` event loop that forwards input to a raw egui-wgpu renderer with zero tao/eframe references, and was owner-confirmed on both platforms.
- SC-2 is proven by a live rusqlite_migration v1 (3 tables, user_version=1) with 4 passing Linux integration tests.
- SC-3 is proven by a typed, panic-free `CredentialStore` trait with two cfg-gated impls; the Linux contract passes locally and the real DPAPI path runs on Windows CI/owner box.
- SC-4 is proven by a clean Linux `cargo build` plus a correctly target-gated manifest (all 8 Windows-only crates isolated) and a windows-latest CI job compiling the full v1 dep set.

Local Linux build+test are green (8 passed / 1 ignored / 0 failed). Windows-only aspects rely on the accepted owner confirmation and the present `build-windows` CI matrix job, which is the correct and documented arrangement for this Windows-only product with a cross-platform core. Phase goal achieved — ready to proceed to Phase 2.

---

_Verified: 2026-07-15T23:05:00Z_
_Verifier: Claude (gsd-verifier)_
