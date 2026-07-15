# Walking Skeleton — Brevly Print

**Phase:** 1
**Generated:** 2026-07-15

## Capability Proven End-to-End

> One sentence: the smallest user-visible capability that exercises the full stack.

Running one binary opens a `winit` + raw-`egui` window and, at startup, creates the app dir,
migrates `state.db`, writes+reads a `config` row, and round-trips a credential through the
`CredentialStore` trait — proving the render/thread model and all persistence infra on a
cross-platform base (Linux dev/test + Windows product).

## Architectural Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Language / runtime | Rust (edition 2024), single native binary | No webview, tiny footprint, always-on reliability (CLAUDE.md constraints) |
| Event loop | `winit 0.30` `ApplicationHandler` + `run_app` (NOT `tao`, NOT `eframe`) | `egui-winit 0.35` requires `winit ^0.30.13`'s trait API; `tao 0.35` uses the incompatible old closure API; `eframe` creates a second event loop (pitfall C2). D-08. |
| GUI | raw `egui 0.35` via `egui-winit` + `egui-wgpu` (features `["winit"]`) | Pure Rust, no webview; renders inside our own event loop so tray-icon + async can share it later. wgpu picks DX12 on Windows, Vulkan/GL on Linux → cross-platform window. |
| Data layer | SQLite via `rusqlite 0.40` (`bundled`) + `rusqlite_migration 2.6` (versioned, `user_version`) | Zero external DLL; ordered migrations are the only safe upgrade path for auto-updated field agents (D-12). SQLite is a dedup/retry tracker, NOT the authoritative queue (Noren owns that). |
| Credential store | `CredentialStore` trait, 2 `cfg`-gated impls: `DpapiCredentialStore` (Windows, `windows-dpapi` `Scope::User`) / `DevFileCredentialStore` (Linux, plaintext DEV-ONLY) | Windows DPAPI is the real secure store; the Linux impl exists ONLY to build/test the trait+error contract on the dev box. Typed `CredentialError` (NotFound/Corrupt) never panics → Phase 2 re-activation hook (M7). D-15/D-16/D-21/D-24. |
| App dir | `dirs::data_dir()` + `BrevlyPrint/`, `create_dir_all` before any file op | `%APPDATA%\Roaming` on Windows, `~/.local/share` on Linux; portable; avoids SQLITE_CANTOPEN (pitfall m2/D-17). |
| Crate layout | `lib` + `bin` split (`src/lib.rs` exposes the stores; `src/main.rs` is the thin event-loop binary) | Lets `cargo test` exercise SQLite schema + credential contract as integration tests (D-06). |
| Cross-platform deps | portable `[dependencies]` + Windows-only `[target.'cfg(windows)'.dependencies]` | Linux never links `windows`/`windows-dpapi`/`tray-icon`/`printers`/`auto-launch`/`velopack`/toast; portable core compiles on both (D-19 revised/D-20). |
| CI | GitHub Actions matrix: `ubuntu-latest` (build+test, fast default gate) + `windows-latest` (build --release + full test, `WGPU_BACKEND=dx12`) | Linux is the day-to-day gate; Windows CI proves the full v1 dep set + real DPAPI (D-03/D-23). No signing until Phase 3. |
| Product target | **Windows-only for v1**; Linux is dev/test parity, NOT a shipping target | D-24. `DevFileCredentialStore` must never ship. |

## Stack Touched in Phase 1

- [x] Project scaffold — Cargo `lib`+`bin`, edition 2024, target-gated dep manifest (01-01)
- [x] Routing/event loop — `winit 0.30` `ApplicationHandler` (01-03)
- [x] Database — real read AND write: `state.db` migrated (v1, 3 tables) + `config` row write/read (01-02 tests, 01-03 runtime)
- [x] UI — interactive egui element: text field + "Aplicar" button → visible label (01-03)
- [x] "Deployment"/full-stack run — documented local run: `cargo run` boots velopack stub → app dir → SQLite → credential round-trip → window (01-03); CI matrix proves both targets build+test

## Out of Scope (Deferred to Later Slices)

> Explicit — prevents future phases re-litigating Phase 1's minimalism.

- Tray icon + green/yellow/red state machine — Phase 3
- Real activation window (serial input, printer/COM dropdown, test-print) — Phase 2
- Autostart (HKCU Run via `auto-launch`) — Phase 2
- Code signing, `vpk` packaging, installer — Phase 3
- Pusher WebSocket client + HMAC channel auth — Phase 4
- Printing (WritePrinter RAW / serial), ESC/POS byte fetch, dedup, ack — Phase 5
- Retry, toast notifications, offline job pull, crash recovery — Phase 6
- SHA256-verified auto-update via Velopack — Phase 7 (Phase 1 only calls the no-op bootstrapper entry)
- **Linux as a shipping product** — deferred idea, not v1 (D-24)

## Subsequent Slice Plan

Each later phase adds one vertical slice on top of this skeleton without altering its
architectural decisions (event loop, store traits, migration pattern, cfg gating):

- Phase 2: Owner enters a serial, selects a printer, test-prints, saves → bound autostarting agent (grows the spike window into the activation screen — D-05)
- Phase 3: Tray icon + invisible runtime + first signed installer
- Phase 4: Pusher event stream (subscribe, HMAC auth, ping/pong, reconnect)
- Phase 5: Event → fetch ESC/POS bytes → WritePrinter/serial → SQLite dedup → ack (adds a `Printer` trait following the CredentialStore cfg pattern — D-21 precedent)
- Phase 6: Retry, toast, offline pull, boot-crash recovery
- Phase 7: Silent SHA256-verified auto-update on reboot
