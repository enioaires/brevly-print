# Walking Skeleton — Brevly Print

**Phase:** 1
**Generated:** 2026-07-15

## Capability Proven End-to-End

A compiled Windows binary opens a raw-`egui` window on a `winit 0.30` event loop, accepts one
real UI interaction (type text → click button → label updates), performs one real SQLite
write+read against a migrated `state.db`, performs one real DPAPI encrypt→decrypt round-trip on
`credential.bin`, and creates `%APPDATA%\BrevlyPrint\` idempotently — the thinnest slice that
exercises the entire GUI + persistence + credential stack the rest of the project builds on.

## Architectural Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Language / runtime | Rust native, Windows-only (`x86_64-pc-windows-msvc`), edition 2024 | Smallest footprint, always-on reliability, no webview (CLAUDE.md constraints; D-01/D-02) |
| Crate layout | `lib` (`brevly_print`) + `bin` (`brevly-print`) split | Lets `cargo test` exercise ConfigStore/CredentialStore/app_dir as real integration tests; `main.rs` stays a thin event-loop wire (D-06) |
| Event loop / windowing | `winit 0.30.13` `ApplicationHandler` trait — **NOT `tao`, NOT `eframe::run_native()`** | `egui-winit 0.35` requires `winit ^0.30.13`; tao 0.35 uses the incompatible old closure API; eframe's second event loop conflicts with the tray loop (pitfall C2). `tray-icon 0.24` supports winit directly. (D-08 revised 2026-07-15) |
| GUI rendering | raw `egui 0.35` via `egui-winit 0.35` + `egui-wgpu 0.35` on `wgpu 29` (DX12 / WARP) | Pure Rust, no webview, ships its own renderer; WARP runs headless in CI. `egui-glow` is the documented fallback if wgpu is flaky (D-11) |
| Data layer | SQLite via `rusqlite 0.40` (bundled) + `rusqlite_migration 2.6` versioned migrations (`user_version`) | Zero external DLL; ordered migrations are the only safe upgrade path for auto-updated field agents (D-12/D-13). SQLite is a dedup/retry tracker, NOT the job queue (Noren owns that) |
| Credential store | `windows-dpapi 0.2` `Scope::User` → `credential.bin`; typed `CredentialError` (thiserror), never panics | DPAPI binds the agentToken to the Windows user SID; corrupt/missing → typed error is the Phase 2 re-activation hook after key loss (M7) (D-15/D-16/D-18) |
| AppData path | `dirs 6` → `%APPDATA%\Roaming\BrevlyPrint\`, `create_dir_all` before any file op | Idempotent init must precede SQLite/credential access (pitfall m2 / D-17) |
| Error handling | `thiserror` for library types, `anyhow` for the binary/glue layer | Typed matchable library errors; contextual top-level errors (D-18) |
| CI | GitHub Actions `windows-latest`, build-only (`cargo build --release` + `cargo test`, `WGPU_BACKEND=dx12`) | Automates SC-4 and catches dep breakage; signing/packaging deferred to Phase 3 (D-03) |
| Directory layout | flat `src/*.rs` modules (`app_dir`, `config_store`, `credential_store`, `spike_window`), `tests/*_test.rs` integration tests | Matches RESEARCH.md §Recommended Project Structure; later phases add modules alongside |

## Stack Touched in Phase 1

- [x] Project scaffold — Cargo lib+bin, full v1 dep set, edition 2024, lint/build (plan 01)
- [x] Event loop / routing — `winit` `ApplicationHandler` drives one real window (plan 03)
- [x] Database — real write AND real read: migrated `state.db`, `config` key/value round-trip (plan 02)
- [x] Credential store — real DPAPI encrypt→decrypt round-trip on `credential.bin` (plan 02)
- [x] UI — one interactive element (text field + button → label) wired into the event loop (plan 03)
- [x] Deployment — documented local full-stack run command: `cargo run` on the Windows box (D-04); CI green as the compile+test gate (plan 01)

## Out of Scope (Deferred to Later Slices)

Explicitly NOT in the skeleton — this list prevents later phases re-litigating Phase 1's minimalism:

- Tray icon + green/yellow/red state machine → Phase 3
- Real activation window (serial input, combined printer/COM dropdown, test-print) → Phase 2
- Serial validation against Noren, agentToken issuance → Phase 2
- Autostart (HKCU Run via auto-launch) → Phase 2
- Authenticode signing, `vpk` packaging, SmartScreen reputation → Phase 3
- Pusher WebSocket client, HMAC channel auth, ping/pong, reconnect → Phase 4
- Any printing (WritePrinter RAW / serialport), ESC/POS byte handling → Phase 5
- Retry queue logic, toast notifications, offline pull, boot-crash recovery → Phase 6
- Velopack auto-update / SHA256 integrity → Phase 7 (only a no-op `VelopackApp::build().run()` startup call is present now)
- Subprocess-fallback IPC design → only if the Phase 1 spike go/no-go (D-10) selects Plan B; then it becomes the Phase 2 activation approach

## Subsequent Slice Plan

Each later phase adds one vertical slice on top of this skeleton without altering its architectural
decisions (event loop = winit, data = rusqlite+migrations, credentials = DPAPI, errors = thiserror):

- **Phase 2 (Activation):** grow the spike window into the real activation screen (serial input +
  printer/COM dropdown + test-print), store the agentToken via CredentialStore, persist config,
  register autostart. *Uses the GUI path selected by the Phase 1 go/no-go (SPIKE-NOTES.md).*
- **Phase 3 (Tray + Runtime + Distributable):** add the tray icon (created on `StartCause::Init`)
  and its state machine to the existing event loop; ship the first Authenticode-signed installer.
- **Phase 4 (Pusher Event Stream):** add the tokio async domain + hand-rolled Pusher client,
  bridged to the main thread via a channel.
- **Phase 5 (Job Pipeline):** add WritePrinter RAW + serialport print paths; `printed_jobs` dedup
  fence and ack ordering on the migrated schema.
- **Phase 6 (Resilience):** activate the `retry_queue` table, toast notifications, offline pull,
  boot-crash recovery.
- **Phase 7 (Auto-Update):** replace the no-op Velopack startup call with real update check +
  SHA256 verification.

---

*Walking Skeleton contract — treat these architectural decisions as fixed for all later slices.*
