---
phase: 01
plan: "01"
subsystem: foundation
tags: [cargo, cross-platform, credential-store, config-store, ci, scaffold]
dependency_graph:
  requires: []
  provides:
    - "Cargo.toml: portable [dependencies] + [target.'cfg(windows)'.dependencies]"
    - "src/lib.rs: lib crate root with pub mod app_dir, config_store, credential_store"
    - "src/app_dir.rs: init_app_dir() -> io::Result<PathBuf>, cross-platform, idempotent"
    - "src/credential_store/{mod,error,dpapi,devfile}.rs: CredentialStore trait + 2 cfg-gated impls"
    - "src/config_store.rs: typed stub (open_and_migrate, set, get) — filled by 01-02/T1"
    - "tests/config_store_test.rs: Wave-0 scaffold (ignore, filled 01-02/T1)"
    - "tests/credential_store_test.rs: Wave-0 Windows-only DPAPI scaffold (ignore, filled 01-02/T2)"
    - "tests/credential_contract_test.rs: Wave-0 portable trait contract scaffold (ignore, filled 01-02/T2)"
    - ".github/workflows/ci.yml: ubuntu-latest + windows-latest CI matrix"
  affects:
    - "01-02: fills config_store stub + credential store tests"
    - "01-03: adds winit event loop to main.rs placeholder"
tech_stack:
  added:
    - "winit 0.30.13 (event loop — replaces tao)"
    - "egui 0.35, egui-winit 0.35, egui-wgpu 0.35 (spike window — Phase 1)"
    - "wgpu 29 (dx12 feature, cross-platform GPU backend)"
    - "rusqlite 0.40 (bundled), rusqlite_migration 2.6 (SQLite + migrations)"
    - "dirs 6 (cross-platform data dir resolution)"
    - "thiserror 2, anyhow 1 (error handling)"
    - "tokio 1, reqwest 0.13 (rustls feature), tokio-tungstenite 0.30 (async runtime + HTTP + WebSocket — Phase 2+)"
    - "hmac 0.13, sha2 0.11 (Pusher auth — Phase 4+)"
    - "serde 1 + serde_json 1 (config serialization)"
    - "tempfile 3 (dev-dependencies)"
    - "[Windows-only] windows 0.62, windows-dpapi 0.2, tray-icon 0.24, printers 2, serialport 4.9, auto-launch 0.6, velopack 1, tauri-winrt-notification 0.8"
  patterns:
    - "lib+bin split (D-06): src/lib.rs exports portable core; src/main.rs is thin binary"
    - "#[cfg(windows)] / #[cfg(not(windows))] gating for platform-specific modules (D-20)"
    - "CredentialStore trait abstraction with 2 cfg-gated impls (D-21)"
    - "serialport placed in [target.'cfg(windows)'.dependencies] to avoid libudev-dev on Linux CI (until Phase 5)"
    - "reqwest 0.13 renamed feature: 'rustls-tls' -> 'rustls' (fixed during Task 1 build)"
key_files:
  created:
    - Cargo.toml
    - Cargo.lock
    - .gitignore
    - src/lib.rs
    - src/main.rs
    - src/app_dir.rs
    - src/config_store.rs
    - src/credential_store/mod.rs
    - src/credential_store/error.rs
    - src/credential_store/dpapi.rs
    - src/credential_store/devfile.rs
    - tests/config_store_test.rs
    - tests/credential_store_test.rs
    - tests/credential_contract_test.rs
    - .github/workflows/ci.yml
  modified: []
decisions:
  - "serialport 4.9 placed in [target.'cfg(windows)'.dependencies]: libudev-dev adds complexity to Linux CI and serialport is not exercised until Phase 5. Move to portable [dependencies] in Phase 5 plan and add libudev-dev to Linux CI apt step at that point."
  - "tests/credential_contract_test.rs added (not in original VALIDATION map): Linux-provable CredentialStore trait + CredentialError contract via DevFileCredentialStore. The original validation map only had the Windows-gated DPAPI tests; this new file enables the trait contract to be tested without a Windows runner."
  - "reqwest 0.13 feature name is 'rustls' not 'rustls-tls': the older name was used in RESEARCH.md but the crate was updated. Fixed inline during Task 1 build."
  - "Task 2 content (init_app_dir full implementation) delivered with Task 1 scaffold: pattern was entirely clear from RESEARCH Pattern 5; no separate commit required."
metrics:
  completed: "2026-07-15"
  tasks: 3
  files_created: 15
  commits: 2
---

# Phase 01 Plan 01: Cross-Platform Cargo Foundation Summary

**One-liner:** Cargo lib+bin project with portable core compiling on Linux, Windows-only crates cfg-gated under `[target.'cfg(windows)'.dependencies]`, CredentialStore trait with DPAPI/DevFile impls, and ubuntu+windows CI matrix.

## What Was Built

This plan stood up the complete cross-platform Cargo foundation for Brevly Print:

**Cargo.toml** with a portable/Windows-only dep split. `[dependencies]` contains winit 0.30, egui 0.35 stack, rusqlite (bundled), tokio, reqwest, dirs, thiserror, anyhow, and Phase 2-7 forward deps (D-19). `[target.'cfg(windows)'.dependencies]` contains windows 0.62, windows-dpapi 0.2, tray-icon 0.24, printers 2, serialport 4.9, auto-launch 0.6, velopack 1, tauri-winrt-notification 0.8. `cargo build` exits 0 on x86_64-unknown-linux-gnu.

**src/app_dir.rs** — `init_app_dir() -> io::Result<PathBuf>` using `dirs::data_dir().join("BrevlyPrint")` + `create_dir_all`. Fully portable, no `#[cfg(windows)]` gate. Two inline tests pass on Linux: existence/is_dir and idempotency (two consecutive calls both Ok).

**src/credential_store/** — `CredentialStore` trait + 2 cfg-gated impls:
- `error.rs`: portable `CredentialError` (NotFound / Corrupt(anyhow::Error) / Io) via thiserror, no panics (D-16)
- `dpapi.rs`: `#[cfg(windows)]` `DpapiCredentialStore` using `windows-dpapi` `Scope::User`, existence check before decrypt
- `devfile.rs`: `#[cfg(not(windows))]` `DevFileCredentialStore` — plaintext, doc-comment: "DEV/TEST ONLY — NOT A SECURE STORE. Never ships." (D-24)
- `mod.rs`: trait declaration + cfg-gated `credential_store(app_dir)` factory function

**src/config_store.rs** — typed stub (`open_and_migrate`, `set`, `get`) with `todo!()` bodies. Type-checks correctly; filled by 01-02/T1.

**Wave-0 test scaffolds** — all compile, all `#[ignore]`, filled by 01-02:
- `tests/config_store_test.rs`: schema v1 + write/read (portable)
- `tests/credential_store_test.rs`: DPAPI round-trip/missing/corrupt (`#![cfg(target_os = "windows")]` first line)
- `tests/credential_contract_test.rs`: portable trait contract via DevFileCredentialStore (NEW addition from cross-platform re-plan; Linux-provable)

**.github/workflows/ci.yml** — CI matrix with two jobs: `build-linux` (ubuntu-latest, apt installs wgpu/winit system deps, `cargo build` + `cargo test`) and `build-windows` (windows-latest, `cargo build --release --target x86_64-pc-windows-msvc` + `cargo test`, `WGPU_BACKEND=dx12`, `RUST_TEST_THREADS=1`). No signing (Phase 3).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] reqwest 0.13 feature renamed from 'rustls-tls' to 'rustls'**
- **Found during:** Task 1 first `cargo build`
- **Issue:** RESEARCH.md specified `features = ["rustls-tls", "json"]` but reqwest 0.13 renamed this feature to `"rustls"`. Build error: "package depends on reqwest with feature rustls-tls but reqwest does not have that feature."
- **Fix:** Updated Cargo.toml to `features = ["rustls", "json"]`
- **Files modified:** `Cargo.toml`
- **Commit:** 84364f8

### Structural Notes (deliberate choices, not bugs)

**serialport in Windows-only deps:** Per plan spec and addendum, serialport 4.9 placed in `[target.'cfg(windows)'.dependencies]` to avoid `libudev-dev` complications on Linux CI. Not exercised until Phase 5. A `# TODO Phase 5` comment in Cargo.toml documents the move-back plan.

**tests/credential_contract_test.rs added:** The original VALIDATION map only had `credential_store_test.rs` (Windows-only DPAPI tests). The cross-platform re-plan added this portable Linux-provable trait contract file as a new requirement. Not windows-gated — exercises trait + CredentialError via `DevFileCredentialStore` on any platform.

**Task 2 content delivered with Task 1:** The full `init_app_dir()` implementation was written during Task 1 scaffold creation (pattern entirely clear from RESEARCH Pattern 5). No separate commit was needed; all acceptance criteria and tests pass.

## Known Stubs

| Stub | File | Lines | Reason |
|------|------|-------|--------|
| `open_and_migrate` | src/config_store.rs | 19 | Intentional — todo!() stub; filled by plan 01-02/T1 |
| `set` | src/config_store.rs | 30 | Intentional — todo!() stub; filled by plan 01-02/T1 |
| `get` | src/config_store.rs | 42 | Intentional — todo!() stub; filled by plan 01-02/T1 |
| minimal main() | src/main.rs | whole file | Intentional placeholder; real event loop added in 01-03 |

These stubs are explicitly required by the plan spec and do not block the plan's goal.

## Linux CI System Dep List

System dependencies specified in `.github/workflows/ci.yml` ubuntu-latest job (from RESEARCH addendum guidance; validated empirically on first CI run):

```
libxkbcommon-dev libwayland-dev libxrandr-dev libxi-dev libgl1-mesa-dev mesa-vulkan-drivers libvulkan1
```

## Success Criteria Met

- [x] SC-4 (Linux half): `cargo build` exits 0 on x86_64-unknown-linux-gnu
- [x] SC-4 (Windows half wired): `build-windows` CI job compiles full v1 dep set on windows-latest
- [x] SC-2 foundation: `init_app_dir()` creates dir idempotently; 2 tests pass on Linux
- [x] SC-3 foundation: `CredentialStore` trait + `CredentialError` + 2 cfg impls compile on Linux
- [x] Wave-0 scaffolds compile on Linux; CI matrix (ubuntu + windows) present
- [x] No `tao` or `eframe` in Cargo.toml (count = 0 confirmed)

## Self-Check: PASSED

Files exist:
- Cargo.toml: FOUND
- Cargo.lock: FOUND
- .gitignore: FOUND
- src/lib.rs: FOUND
- src/main.rs: FOUND
- src/app_dir.rs: FOUND
- src/config_store.rs: FOUND
- src/credential_store/mod.rs: FOUND
- src/credential_store/error.rs: FOUND
- src/credential_store/dpapi.rs: FOUND
- src/credential_store/devfile.rs: FOUND
- tests/config_store_test.rs: FOUND
- tests/credential_store_test.rs: FOUND
- tests/credential_contract_test.rs: FOUND
- .github/workflows/ci.yml: FOUND

Commits verified:
- 84364f8: feat(01-01): cross-platform Cargo.toml + lib/bin scaffold compiling on Linux
- 71e6ac2: feat(01-01): Wave-0 test scaffolds + ubuntu+windows CI matrix
