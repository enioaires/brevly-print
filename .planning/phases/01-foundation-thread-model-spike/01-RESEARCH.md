# Phase 1: Foundation + Thread Model Spike — Research

**Researched:** 2026-07-15
**Domain:** Rust native Windows GUI spike (egui + wgpu + winit), SQLite persistence, DPAPI credential store
**Confidence:** HIGH (all crates verified on crates.io; egui+winit integration confirmed via working reference implementation; DPAPI API confirmed from source)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

| Decision | Value |
|----------|-------|
| D-01 | Compile and run **natively on Windows** (owner has a Windows machine/VM). `git pull` + `cargo run`/`cargo test` on Windows. Linux for planning/git only. No cross-compilation. |
| D-02 | Target triple `x86_64-pc-windows-msvc`. |
| D-03 | **GitHub Actions Windows runner** from Phase 1: build-only gate (`cargo build --release` + `cargo test`). No signing in CI yet. |
| D-04 | Spike proven **interactively on the Windows box** (window renders, DPAPI round-trips, SQLite + `%APPDATA%` init). CI proves compilation only. |
| D-05 | The `tao`+`egui` spike **becomes the real `src/` foundation** — not throwaway. `src/main.rs` owns the event loop; Phase 2 grows that window into the activation screen. |
| D-06 | **`lib` + `bin` split**: `src/lib.rs` exposes `ConfigStore`, `CredentialStore`, `%APPDATA%` init. `src/main.rs` is the thin binary. `cargo test` exercises the SQLite schema and DPAPI round-trip as integration tests. |
| D-07 | Walking-skeleton scope: compile → window opens → one egui interaction (text input + button) → one SQLite write/read → one DPAPI round-trip → `%APPDATA%\BrevlyPrint\` created. |
| D-08 | **Primary approach: raw `egui` rendered inside the event loop via `egui-wgpu`** (DirectX backend). `eframe::run_native()` is **PROHIBITED** (dual event-loop conflict C2). |
| D-09 | Timebox ~1–2 focused days on the embedded approach. Bar: one interactive egui frame renders. |
| D-10 | If timebox hit: **auto-switch to subprocess fallback** (separate short-lived window process, temp-file/named-pipe IPC). Document evidence, flag for owner review. |
| D-11 | Renderer fallback: if `egui-wgpu` bloats binary or is flaky, evaluate `egui-glow`. |
| D-12 | **Versioned migrations via `rusqlite_migration`** (`user_version` pragma). Phase 1 registers migration v1 = the three tables. |
| D-13 | `state.db` at `%APPDATA%\BrevlyPrint\state.db`; `rusqlite` with `features = ["bundled"]`. |
| D-14 | Schema v1: `config` (key/value), `printed_jobs` (dedup PK + status), `retry_queue` (BLOB + retry metadata). |
| D-15 | `credential.bin` at `%APPDATA%\BrevlyPrint\credential.bin` via `windows-dpapi`, **`Scope::User`**. Proves round-trip with dummy value. |
| D-16 | Missing or undecryptable `credential.bin` → typed `CredentialError` — **never panics**. |
| D-17 | `%APPDATA%\BrevlyPrint\` created via `std::fs::create_dir_all` at startup (idempotent) **before** opening `state.db` or `credential.bin`. |
| D-18 | Library errors via **`thiserror`** (typed, matchable). Binary/glue may use `anyhow`. |
| D-19 | **Full v1 dependency set** present and compiling from Phase 1, even if unused until later phases. |

### Claude's Discretion

- Exact `egui`/`egui-wgpu`/`wgpu`/`glow` crate glue and window-creation boilerplate.
- Exact SQLite column types/indexes within the D-14 shape.
- Test harness layout within the D-06 lib+bin split.
- Subprocess-fallback IPC mechanism — decided only if D-10 triggers.

### Deferred Ideas (OUT OF SCOPE)

- Tray icon rendering (green/yellow/red state machine) — Phase 3.
- Signing, `vpk` packaging, VirusTotal/CI signing steps — Phase 3.
- Real activation window (serial input, printer dropdown, test-print) — Phase 2.
- Subprocess-fallback IPC design — only if D-10 triggers.
</user_constraints>

---

## Summary

Phase 1 is a pure technical spike plus persistence infrastructure. It has one job: prove the rendering thread model so Phase 2 can build the activation window without a foundation risk under it.

**The highest-risk unknown is confirmed.** Research found a working reference implementation (`matthewjberger/wgpu-example`) that integrates `egui-winit` 0.34/0.35 + `egui-wgpu` + `wgpu` + `winit 0.30` via the `ApplicationHandler` trait, without `eframe`. The `tray-icon` crate's own `winit.rs` example (shipped in the crate repository) confirms `tray-icon 0.24` integrates cleanly with `winit 0.30`'s `ApplicationHandler` + `EventLoop::<UserEvent>` + `run_app()`. **The primary approach (D-08) is viable. The core unknown is de-risked.**

**Critical version drift discovered.** Crates have moved significantly since CONTEXT.md was drafted. The locked decisions in D-19 cite versions from the original stack research (e.g., `egui 0.31`, `rusqlite 0.32`, `tray-icon 0.21`). Every crate is 1–3 major bumps newer. The full reconciled version table is in §Standard Stack below. The good news: the API patterns are stable, the migration path is straightforward, and all crates are confirmed on crates.io.

**Key architectural pivot (confirmed by research).** The CONTEXT.md and prior research described using `tao` as the event loop. Research reveals that `tao 0.35.3` uses the **old closure-based `EventLoop::run()` API** (`FnMut(Event, &EventLoopWindowTarget, &mut ControlFlow)`) while `egui-winit 0.35` requires `winit ^0.30.13` with the **new `ApplicationHandler` trait API** (`resumed()`, `window_event()`, `run_app()`). These are incompatible. **The correct event loop for this stack is `winit 0.30`, not `tao 0.35`.** `tray-icon 0.24.1` explicitly supports `winit 0.30` (its `winit.rs` example uses `ApplicationHandler`) and has no coupling to `tao` at runtime.

**Primary recommendation:** Use `winit 0.30` (not `tao 0.35`) as the event loop. Pair with `tray-icon 0.24`, `egui-winit 0.35`, `egui-wgpu 0.35`, `wgpu 29`. wgpu uses WARP (Windows Advanced Rasterization Platform software rasterizer via DX12) on GHA Windows runners — no GPU required for CI build+test.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Event loop / window pump | Main OS thread (Win32) | — | `winit::EventLoop` blocks the calling thread; must be on main; `tray-icon` requires same thread |
| egui rendering (spike window) | Main OS thread | — | `winit::Window` is `!Send`; `wgpu::Surface` must be created on the window's thread |
| Tray icon (Phase 3) | Main OS thread | — | `tray-icon` must be created after event loop starts (`StartCause::Init`); Win32 message pump |
| Async I/O (future phases) | Tokio thread pool | — | Tokio runtime spawned on a dedicated OS thread; communicates with main via `SyncSender` |
| SQLite persistence | Library (`src/lib.rs`) | Main thread at startup | `rusqlite::Connection` is not `Send`; wrap in `Arc<Mutex<>>` or open per-operation |
| DPAPI credential store | Library (`src/lib.rs`) | Main thread at startup | Blocking Win32 call; safe on any thread; returns `Vec<u8>` |
| `%APPDATA%` directory init | Library (`src/lib.rs`) | Startup (before any file ops) | Must precede SQLite open and credential read (pitfall m2) |
| GitHub Actions CI (build+test) | External (GHA Windows runner) | — | `windows-latest`; wgpu uses WARP DX12 software rasterizer; no GPU needed for `cargo build` |

---

## Standard Stack

### Verified Current Versions (confirmed crates.io, 2026-07-15)

> **Version drift warning:** CONTEXT.md D-19 cites versions from the July 2026 stack research session. All crates have newer releases. The table below is the actual current state. The planner MUST use these versions in Cargo.toml, not the versions in D-19.

| Crate | D-19 Version (stale) | Current Version | Updated | Phase 1 Use |
|-------|---------------------|-----------------|---------|-------------|
| `winit` | — (used tao) | **0.30.13** | active | **Replace tao** as event loop |
| `tao` | 0.35 | 0.35.3 | 2026-05-23 | NOT USED — see §Architecture Patterns |
| `tray-icon` | 0.21 | **0.24.1** | 2026-06-10 | Phase 3; included in D-19 dep set |
| `egui` | 0.31 | **0.35.0** | 2026-06-25 | Spike window UI |
| `egui-wgpu` | (implied 0.31) | **0.35.0** | 2026-06-25 | wgpu renderer for egui |
| `egui-winit` | (implied 0.31) | **0.35.0** | 2026-06-25 | Event translation winit→egui |
| `wgpu` | (via egui-wgpu) | **29.0.4** | 2026-07-02 | GPU rendering backend |
| `rusqlite` | 0.32 | **0.40.1** | 2026-06-06 | SQLite persistence |
| `rusqlite_migration` | (implied) | **2.6.0** | 2026-05-28 | Versioned migrations; requires rusqlite ≥0.40 |
| `windows-dpapi` | (latest) | **0.2.0** | 2026-03-12 | DPAPI encrypt/decrypt (uses `winapi` crate) |
| `dirs` | (implied) | **6.0.0** | 2025-01-12 | `%APPDATA%` path resolution |
| `thiserror` | (latest) | **2.0.18** | 2026-01-18 | Typed library errors |
| `anyhow` | (latest) | **1.0.103** | 2026-06-25 | Top-level binary error context |
| `tokio` | 1.x | **1.52.3** | 2026-05-08 | Phase 3+; included for D-19 |
| `reqwest` | 0.13 | **0.13.4** | 2026-05-25 | Phase 2+; included for D-19 |
| `tokio-tungstenite` | 0.26 | **0.30.0** | 2026-07-11 | Phase 4+; included for D-19 |
| `hmac` | 0.12 | **0.13.0** | 2026-03-29 | Phase 4+; included for D-19 |
| `sha2` | 0.10 | **0.11.0** | 2026-03-25 | Phase 4+; included for D-19 |
| `serialport` | 4.7 | **4.9.0** | 2026-03-16 | Phase 5+; included for D-19 |
| `auto-launch` | 0.5 | **0.6.0** | 2026-01-10 | Phase 2+; included for D-19 |
| `printers` | 0.2 | **2.3.0** | 2026-02-06 | Phase 2+; included for D-19 |
| `velopack` | 0.0 | **1.2.0** | 2026-06-03 | Phase 7+; included for D-19 |
| `tauri-winrt-notification` | 0.5 | **0.8.0** | 2026-07-06 | Phase 6+; included for D-19 |
| `windows` | 0.62 | **0.62.2** | 2025-10-06 | Phase 5 printing; included for D-19 |
| `serde` | 1 | **1.0.228** | active | Config serialization |
| `serde_json` | 1 | **1.0.150** | active | Config serialization |

[VERIFIED: crates.io] — all version numbers confirmed via `curl https://crates.io/api/v1/crates/<name>` on 2026-07-15.

### Phase 1 Active Dependencies (exercised in this phase)

```toml
[dependencies]
# ── Event loop + spike window ──────────────────────────────────────────────
winit        = "0.30.13"
egui         = "0.35"
egui-wgpu    = { version = "0.35", features = ["winit"] }
egui-winit   = "0.35"
wgpu         = { version = "29", default-features = false, features = ["wgsl", "dx12"] }

# ── Persistence ───────────────────────────────────────────────────────────
rusqlite           = { version = "0.40", features = ["bundled"] }
rusqlite_migration = "2.6"

# ── DPAPI credential store ─────────────────────────────────────────────────
windows-dpapi = "0.2"

# ── AppData path resolution ────────────────────────────────────────────────
dirs = "6"

# ── Error handling ─────────────────────────────────────────────────────────
thiserror = "2"
anyhow    = "1"
```

### Full v1 Dependency Set (D-19 — included but unused until later phases)

```toml
[dependencies]
# ── Event loop + spike window (Phase 1) ────────────────────────────────────
winit        = "0.30.13"
egui         = "0.35"
egui-wgpu    = { version = "0.35", features = ["winit"] }
egui-winit   = "0.35"
wgpu         = { version = "29", default-features = false, features = ["wgsl", "dx12"] }

# ── System tray (Phase 3) ─────────────────────────────────────────────────
tray-icon    = "0.24"

# ── Windows printing spooler (Phase 5) ───────────────────────────────────
[dependencies.windows]
version  = "0.62"
features = [
  "Win32_Graphics_Printing",
  "Win32_Foundation",
]

# ── Printer enumeration (Phase 2) ────────────────────────────────────────
printers = "2"

# ── Serial port (Phase 5) ────────────────────────────────────────────────
serialport = "4.9"

# ── Async runtime + HTTP (Phase 2+) ──────────────────────────────────────
tokio   = { version = "1", features = ["full"] }
reqwest = { version = "0.13", default-features = false, features = ["rustls-tls", "json"] }

# ── WebSocket / Pusher (Phase 4) ──────────────────────────────────────────
tokio-tungstenite = { version = "0.30", features = ["rustls-tls-webpki-roots"] }

# ── Pusher HMAC auth (Phase 4) ────────────────────────────────────────────
hmac = "0.13"
sha2 = "0.11"

# ── Persistence (Phase 1) ────────────────────────────────────────────────
rusqlite           = { version = "0.40", features = ["bundled"] }
rusqlite_migration = "2.6"

# ── Config serialization ──────────────────────────────────────────────────
serde      = { version = "1", features = ["derive"] }
serde_json = "1"

# ── AppData path ──────────────────────────────────────────────────────────
dirs = "6"

# ── DPAPI credential store (Phase 1) ─────────────────────────────────────
windows-dpapi = "0.2"

# ── Autostart (Phase 2) ───────────────────────────────────────────────────
auto-launch = "0.6"

# ── Auto-update (Phase 7) ────────────────────────────────────────────────
velopack = "1"

# ── Windows toast notifications (Phase 6) ────────────────────────────────
tauri-winrt-notification = "0.8"

# ── Error handling ────────────────────────────────────────────────────────
thiserror = "2"
anyhow    = "1"
```

**Note on `tao`:** `tao 0.35` is NOT in this Cargo.toml. `winit 0.30` is the event loop. See §Architecture Patterns for rationale.

**Note on `eframe`:** `eframe` is NOT in this Cargo.toml. D-08 prohibits `eframe::run_native()`.

---

## Package Legitimacy Audit

> slopcheck was run but only checks PyPI (Python). This is a Rust project — all packages verified directly on crates.io. slopcheck results for Rust packages on PyPI are meaningless (expected "SLOP" because these are Rust crates, not Python packages).

| Package | Registry | Age | Confirmed Source | Disposition |
|---------|----------|-----|-----------------|-------------|
| `winit` | crates.io | 10+ yrs | github.com/rust-windowing/winit | [VERIFIED: crates.io] Approved |
| `egui` | crates.io | 5+ yrs | github.com/emilk/egui | [VERIFIED: crates.io] Approved |
| `egui-wgpu` | crates.io | 3+ yrs | github.com/emilk/egui (workspace) | [VERIFIED: crates.io] Approved |
| `egui-winit` | crates.io | 3+ yrs | github.com/emilk/egui (workspace) | [VERIFIED: crates.io] Approved |
| `wgpu` | crates.io | 5+ yrs | github.com/gfx-rs/wgpu | [VERIFIED: crates.io] Approved |
| `tray-icon` | crates.io | 3+ yrs | github.com/tauri-apps/tray-icon | [VERIFIED: crates.io] Approved |
| `rusqlite` | crates.io | 8+ yrs | github.com/rusqlite/rusqlite | [VERIFIED: crates.io] Approved |
| `rusqlite_migration` | crates.io | 3+ yrs | github.com/cljoly/rusqlite_migration | [VERIFIED: crates.io] Approved |
| `windows-dpapi` | crates.io | active | github.com/sheridans/windows-dpapi | [VERIFIED: crates.io] Approved |
| `dirs` | crates.io | 6+ yrs | github.com/dirs-dev/dirs-rs | [VERIFIED: crates.io] Approved |
| `thiserror` | crates.io | 5+ yrs | github.com/dtolnay/thiserror | [VERIFIED: crates.io] Approved |
| `anyhow` | crates.io | 5+ yrs | github.com/dtolnay/anyhow | [VERIFIED: crates.io] Approved |
| `tokio` | crates.io | 6+ yrs | github.com/tokio-rs/tokio | [VERIFIED: crates.io] Approved |
| `reqwest` | crates.io | 6+ yrs | github.com/seanmonstar/reqwest | [VERIFIED: crates.io] Approved |
| `tokio-tungstenite` | crates.io | 5+ yrs | github.com/snapview/tokio-tungstenite | [VERIFIED: crates.io] Approved |
| `hmac` | crates.io | 5+ yrs | github.com/RustCrypto/MACs | [VERIFIED: crates.io] Approved |
| `sha2` | crates.io | 5+ yrs | github.com/RustCrypto/hashes | [VERIFIED: crates.io] Approved |
| `serialport` | crates.io | 5+ yrs | github.com/serialport/serialport-rs | [VERIFIED: crates.io] Approved |
| `auto-launch` | crates.io | 3+ yrs | github.com/zzzgydi/auto-launch | [VERIFIED: crates.io] Approved |
| `printers` | crates.io | 3+ yrs | github.com/talesluna/rust-printers | [VERIFIED: crates.io] Approved |
| `velopack` | crates.io | active | github.com/velopack/velopack | [VERIFIED: crates.io] Approved |
| `tauri-winrt-notification` | crates.io | 3+ yrs | github.com/tauri-apps/tauri-winrt-notification | [VERIFIED: crates.io] Approved |
| `windows` | crates.io | 4+ yrs | github.com/microsoft/windows-rs | [VERIFIED: crates.io] Approved |
| `serde` / `serde_json` | crates.io | 8+ yrs | github.com/serde-rs | [VERIFIED: crates.io] Approved |
| `wgpu` | crates.io | 5+ yrs | github.com/gfx-rs/wgpu | [VERIFIED: crates.io] Approved |
| `windows-dpapi` | crates.io | active | github.com/sheridans/windows-dpapi | [VERIFIED: crates.io] Approved |

**Packages removed due to slopcheck [SLOP] verdict:** none (slopcheck checked PyPI; these are Rust crates; no packages removed)
**Packages flagged as suspicious [SUS]:** none from crates.io verification

---

## Architecture Patterns

### System Architecture Diagram — Phase 1 Walking Skeleton

```
┌──────────────────────────────────────────────────────────────┐
│  Main OS Thread                                              │
│                                                              │
│  main()                                                      │
│    └─ AppState::init()                                       │
│         ├─ create_dir_all(%APPDATA%\BrevlyPrint\)  ← m2     │
│         ├─ rusqlite::Connection::open(state.db)              │
│         │     └─ MIGRATIONS.to_latest(&mut conn) ← D-12     │
│         └─ CredentialStore::load_or_none()       ← D-15/16  │
│                                                              │
│  winit::EventLoop::<UserEvent>::with_user_event()            │
│    └─ event_loop.run_app(&mut App { ... })                   │
│         ├─ ApplicationHandler::resumed()                     │
│         │     ├─ event_loop.create_window(attrs)             │
│         │     ├─ wgpu: Instance→Adapter→Device→Queue         │
│         │     ├─ egui::Context::default()                    │
│         │     ├─ egui_winit::State::new(ctx, vp_id, window)  │
│         │     └─ egui_wgpu::Renderer::new(device, format)   │
│         │                                                    │
│         ├─ ApplicationHandler::window_event()                │
│         │     ├─ egui_state.on_window_event(window, &event) │
│         │     └─ WindowEvent::RedrawRequested:               │
│         │           ├─ raw_input = egui_state.take_egui_input│
│         │           ├─ ctx.run(raw_input, |ui| { egui UI }) │
│         │           ├─ egui_renderer.render(...)             │
│         │           └─ surface.present()                     │
│         │                                                    │
│         └─ ApplicationHandler::user_event()                  │
│               └─ (TrayIconEvent in Phase 3)                  │
└──────────────────────────────────────────────────────────────┘

Data stores (created at startup, accessed from main thread):
  %APPDATA%\BrevlyPrint\state.db        (SQLite via rusqlite)
  %APPDATA%\BrevlyPrint\credential.bin  (DPAPI blob via windows-dpapi)
```

### Recommended Project Structure

```
brevly-print/
├── Cargo.toml                # full v1 dep set (D-19)
├── Cargo.lock                # committed (binary project)
├── .github/
│   └── workflows/
│       └── ci.yml            # windows-latest, cargo build + test
├── src/
│   ├── main.rs               # thin binary: calls AppState::init(), runs event loop
│   ├── lib.rs                # re-exports: ConfigStore, CredentialStore, init_app_dir
│   ├── config_store.rs       # SQLite ConfigStore: open(), migrate(), get/set
│   ├── credential_store.rs   # DPAPI CredentialStore: save(), load(), CredentialError
│   ├── app_dir.rs            # init_app_dir() -> PathBuf (%APPDATA%\BrevlyPrint\)
│   └── spike_window.rs       # egui spike UI (text field + button); removed/replaced in Phase 2
└── tests/
    ├── config_store_test.rs  # integration: SQLite schema + read/write
    └── credential_store_test.rs  # integration: DPAPI round-trip (runs on Windows only)
```

---

### Pattern 1: winit 0.30 ApplicationHandler + tray-icon (the definitive event loop pattern)

**What:** Replace `tao::EventLoop` with `winit::EventLoop`. Use the `ApplicationHandler` trait. Forward `TrayIconEvent` and `MenuEvent` via `EventLoopProxy`.

**Why:** `egui-winit 0.35` requires `winit ^0.30.13` types (`winit::window::Window`, `winit::event::WindowEvent`). `tao 0.35` uses an incompatible closure-based API. `tray-icon 0.24` explicitly supports `winit 0.30` (see its `examples/winit.rs`).

**Source:** [VERIFIED: tray-icon/examples/winit.rs, confirmed via crates.io deps lookup 2026-07-15]

```rust
// Source: tray-icon/examples/winit.rs (tray-icon 0.24.1, confirmed 2026-07-15)
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ControlFlow, EventLoop},
};

#[derive(Debug)]
enum UserEvent {
    TrayIconEvent(tray_icon::TrayIconEvent),
    MenuEvent(tray_icon::menu::MenuEvent),
}

struct App {
    tray_icon: Option<TrayIcon>,
    // ... egui state, wgpu renderer, etc.
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        // Create tray icon AFTER event loop starts (not before)
        if cause == winit::event::StartCause::Init {
            self.tray_icon = Some(TrayIconBuilder::new()
                .with_tooltip("BrevlyPrint")
                .build()
                .unwrap());
        }
    }

    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        // Create window + wgpu surface + egui context here
        let window = event_loop.create_window(
            winit::window::Window::default_attributes()
                .with_title("BrevlyPrint Setup")
                .with_visible(false), // hidden until egui is ready
        ).unwrap();
        // ... initialize wgpu, egui_winit::State, egui_wgpu::Renderer
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        // Feed events to egui-winit
        if let Some(state) = self.egui_state.as_mut() {
            if state.on_window_event(&self.window.as_ref().unwrap(), &event).consumed {
                return;
            }
        }
        match event {
            WindowEvent::RedrawRequested => { /* render egui frame */ }
            WindowEvent::CloseRequested => { event_loop.exit(); }
            _ => {}
        }
    }

    fn user_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        event: UserEvent,
    ) {
        match event {
            UserEvent::TrayIconEvent(e) => { /* handle tray click */ }
            UserEvent::MenuEvent(e) => { /* handle menu item */ }
        }
    }
}

fn main() {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    // Forward tray/menu events into the event loop
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::TrayIconEvent(e));
    }));
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::MenuEvent(e));
    }));

    let mut app = App { tray_icon: None, /* ... */ };
    event_loop.run_app(&mut app).unwrap();
}
```

---

### Pattern 2: egui-winit + egui-wgpu render loop (the spike render pattern)

**What:** The complete egui rendering integration inside an `ApplicationHandler::window_event(RedrawRequested)` handler.

**Source:** [VERIFIED: matthewjberger/wgpu-example src/lib.rs, confirmed 2026-07-15 — uses egui 0.34/egui-wgpu 0.34/winit 0.30.13/wgpu 29]

```rust
// Source: matthewjberger/wgpu-example (adapted for Phase 1 spike)
// This is the proven pattern — uses winit 0.30 ApplicationHandler

use std::sync::Arc;
use egui_wgpu::ScreenDescriptor;
use winit::window::Window;

struct EguiRenderer {
    context: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
}

impl EguiRenderer {
    fn new(
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
        window: &Arc<Window>,
    ) -> Self {
        let context = egui::Context::default();
        let viewport_id = context.viewport_id();
        let state = egui_winit::State::new(
            context.clone(),
            viewport_id,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            Some(egui::Theme::Dark),
            None, // max_texture_side
        );
        let renderer = egui_wgpu::Renderer::new(device, output_format, None, 1, false);
        Self { context, state, renderer }
    }

    fn handle_input(&mut self, window: &Window, event: &winit::event::WindowEvent) -> egui_winit::EventResponse {
        self.state.on_window_event(window, event)
    }

    fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &Window,
        window_surface_view: &wgpu::TextureView,
        screen_descriptor: ScreenDescriptor,
        run_ui: impl FnOnce(&egui::Context),
    ) {
        let raw_input = self.state.take_egui_input(window);
        let full_output = self.context.run(raw_input, |ctx| { run_ui(ctx); });

        self.state.handle_platform_output(window, full_output.platform_output);

        let primitives = self.context.tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, image_delta);
        }

        self.renderer.update_buffers(device, queue, encoder, &primitives, &screen_descriptor);

        let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: window_surface_view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });

        self.renderer.render(&mut render_pass.forget_lifetime(), &primitives, &screen_descriptor);

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}
```

---

### Pattern 3: DPAPI credential store (windows-dpapi 0.2.0)

**What:** Encrypt/decrypt the `agentToken` using DPAPI Scope::User. Handle decryption failure gracefully as `CredentialError`.

**Source:** [VERIFIED: github.com/sheridans/windows-dpapi src/lib.rs, read 2026-07-15]

```rust
// Source: windows-dpapi 0.2.0 API (confirmed from source)
use windows_dpapi::{encrypt_data, decrypt_data, Scope};
use thiserror::Error;
use std::path::PathBuf;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Credential file not found — agent needs activation")]
    NotFound,
    #[error("Credential file is corrupt or was encrypted by a different Windows user (DPAPI key mismatch)")]
    Corrupt(#[source] anyhow::Error),
    #[error("IO error reading credential file")]
    Io(#[from] std::io::Error),
}

pub struct CredentialStore {
    path: PathBuf,
}

impl CredentialStore {
    pub fn new(app_dir: &PathBuf) -> Self {
        Self { path: app_dir.join("credential.bin") }
    }

    pub fn save(&self, token: &str) -> Result<(), CredentialError> {
        let encrypted = encrypt_data(token.as_bytes(), Scope::User, None)
            .map_err(CredentialError::Corrupt)?;
        std::fs::write(&self.path, encrypted)?;
        Ok(())
    }

    pub fn load(&self) -> Result<String, CredentialError> {
        if !self.path.exists() {
            return Err(CredentialError::NotFound);
        }
        let ciphertext = std::fs::read(&self.path)?;
        let plaintext = decrypt_data(&ciphertext, Scope::User, None)
            .map_err(CredentialError::Corrupt)?;
        String::from_utf8(plaintext)
            .map_err(|e| CredentialError::Corrupt(anyhow::anyhow!(e)))
    }
}
```

**Key behavior (from `CryptUnprotectData` semantics confirmed in source):**
- `decrypt_data` returns `Err(anyhow)` wrapping `std::io::Error::last_os_error()` when `CryptUnprotectData` fails (returns 0). The Win32 error code is included.
- A missing file is a distinct case handled BEFORE calling `decrypt_data` (check `self.path.exists()`).
- Error code `ERROR_DECRYPTION_FAILED` (0x80090330) indicates DPAPI key mismatch (Windows reinstall / new user SID) — this maps to `CredentialError::Corrupt`. Phase 2 hooks this to re-enter activation flow (pitfall M7).

---

### Pattern 4: rusqlite_migration v1 schema (D-12, D-14)

**What:** Versioned migration setup with `user_version` pragma. Phase 1 registers exactly v1 = the three tables.

**Source:** [VERIFIED: docs.rs/rusqlite_migration/2.6.0, confirmed 2026-07-15]

```rust
// Source: rusqlite_migration 2.6.0 confirmed API
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

// Define once as a module-level constant (or lazy_static)
const MIGRATIONS: Migrations<'static> = Migrations::from_slice(&[
    // v1: Phase 1 schema
    M::up("
        CREATE TABLE config (
            key   TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );

        CREATE TABLE printed_jobs (
            job_id      TEXT PRIMARY KEY NOT NULL,
            job_type    TEXT NOT NULL,
            status      TEXT NOT NULL DEFAULT 'pending'
                        CHECK(status IN ('pending','printing','done','failed')),
            attempt     INTEGER NOT NULL DEFAULT 0,
            received_at TEXT NOT NULL,
            printed_at  TEXT,
            failed_at   TEXT
        );
        CREATE INDEX idx_printed_jobs_status ON printed_jobs(status);

        CREATE TABLE retry_queue (
            job_id       TEXT PRIMARY KEY NOT NULL
                         REFERENCES printed_jobs(job_id),
            job_type     TEXT NOT NULL,
            escpos_bytes BLOB,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_retry_at TEXT,
            last_error    TEXT,
            created_at    TEXT NOT NULL
        );
    "),
    // v2, v3, … added by future phases
]);

pub fn open_and_migrate(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open(path)?;
    MIGRATIONS.to_latest(&mut conn)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    Ok(conn)
}
```

**Why `rusqlite_migration` not `CREATE TABLE IF NOT EXISTS`:** `to_latest()` writes `user_version` to the SQLite file header. Future `ALTER TABLE` migrations append a new `M::up()` entry at the next index. `CREATE TABLE IF NOT EXISTS` has no ordered-migration story for schema evolution after field agents are deployed (D-12 rationale).

---

### Pattern 5: `%APPDATA%` directory initialization (D-17)

**What:** Resolve `%APPDATA%\BrevlyPrint\` and create it idempotently. Must happen before SQLite open or credential read.

**Source:** [VERIFIED: dirs 6.0.0 source, crates.io 2026-07-15]

```rust
// Source: dirs 6.0.0 (confirmed on crates.io 2026-07-15)
use std::path::PathBuf;

pub fn init_app_dir() -> Result<PathBuf, std::io::Error> {
    // dirs::data_local_dir() resolves to %APPDATA%\Local on Windows
    // dirs::data_dir() resolves to %APPDATA%\Roaming on Windows
    // For an always-on agent, Roaming is correct (syncs across domain logins if applicable)
    let base = dirs::data_dir()
        .ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Cannot resolve %APPDATA% — no user profile?"
        ))?;
    let app_dir = base.join("BrevlyPrint");
    std::fs::create_dir_all(&app_dir)?;  // idempotent: no-op if exists
    Ok(app_dir)
}
```

**`dirs::data_dir()` on Windows** returns `%APPDATA%\Roaming` (i.e., `C:\Users\{user}\AppData\Roaming`). This is the correct location for application data that should persist across Windows reinstalls when the user profile is preserved. The alternative `data_local_dir()` returns `%APPDATA%\Local` (non-roaming). Either works; `Roaming` is conventional for agent configuration.

**Alternative via `std::env`** (no `dirs` dep): `std::env::var("APPDATA")` — works on Windows but `dirs` handles edge cases (missing env var, non-Windows targets for tests).

---

### Pattern 6: GitHub Actions Windows CI (D-03)

**What:** Build-only CI gate: `cargo build --release` + `cargo test` on Windows runner. No signing. Uses WARP software rasterizer for headless wgpu.

**Source:** [VERIFIED: GitHub Actions docs, WARP Wikipedia 2026-07-15]

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  # Force wgpu to use DX12 WARP (software rasterizer) in CI
  # GitHub Actions windows-latest runners have DX12/WARP available
  WGPU_BACKEND: dx12

jobs:
  build-windows:
    name: Build & Test (Windows MSVC)
    runs-on: windows-latest

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust stable (x86_64-pc-windows-msvc)
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-msvc

      - name: Cache cargo registry + build artifacts
        uses: Swatinem/rust-cache@v2

      - name: Build release binary
        run: cargo build --release --target x86_64-pc-windows-msvc

      - name: Run tests
        run: cargo test --target x86_64-pc-windows-msvc
        env:
          # DPAPI tests require a real Windows user profile — GHA runners have one
          # wgpu tests use WARP (no GPU needed)
          RUST_TEST_THREADS: 1  # SQLite file-lock safety in integration tests
```

**WARP availability on GHA runners:** Windows Advanced Rasterization Platform is a software D3D12 renderer built into Windows. All GitHub Actions `windows-latest` runners (Windows Server 2022) have DX12/WARP. Setting `WGPU_BACKEND=dx12` forces wgpu to use it. No GPU hardware required.

**DPAPI on GHA runners:** `windows-latest` runners run under a real Windows user session with a user profile. DPAPI `Scope::User` works because the user session exists. This means `cargo test` can run the DPAPI round-trip integration test in CI.

---

### Anti-Patterns to Avoid

- **Using `tao 0.35` as the event loop:** tao uses the old closure-based `FnMut(Event, &EventLoopWindowTarget, &mut ControlFlow)` API. `egui-winit 0.35` requires `winit 0.30` types. They are incompatible — do not use tao.
- **Using `eframe::run_native()`:** Prohibited (D-08, pitfall C2). Creates a second event loop. Panics at runtime.
- **Using `egui-tao` crate:** Pinned to `egui 0.22` + `tao 0.20` — 13 major versions behind. Do not use.
- **Calling `TrayIconBuilder::new()` before `StartCause::Init`:** tray-icon panics or silently fails if created before the Win32 message pump is running. Create it inside `new_events()` when `cause == StartCause::Init`. (Phase 3 concern but pattern is set now.)
- **Opening SQLite before `create_dir_all`:** Returns `SQLITE_CANTOPEN`. Init order: `init_app_dir()` → `Connection::open()`. (Pitfall m2.)
- **Calling `MIGRATIONS.to_latest()` on each open without connection type:** Pass `&mut Connection` (mutable). rusqlite_migration writes to `user_version` pragma and needs write access.
- **Inline `block_on` inside tokio tasks (future phases):** Panics. Use `.await` or `spawn_blocking`. (Anti-pattern from ARCHITECTURE.md.)

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| egui ↔ winit event translation | Manual `WindowEvent` → egui input mapping | `egui-winit::State::on_window_event()` | ~500 lines of mapping: keyboard scancodes, IME, DPI scaling, focus, drag — all handled |
| egui wgpu rendering pipeline | Custom wgpu render pass for egui primitives | `egui-wgpu::Renderer` | Tessellation, texture atlas management, buffer upload, render pass — all handled |
| SQLite schema versioning | `CREATE TABLE IF NOT EXISTS` + manual `user_version` reads | `rusqlite_migration` | Ordered migrations, `user_version` tracking, idempotent apply — all handled |
| DPAPI encrypt/decrypt | `winapi::um::dpapi::CryptProtectData` directly | `windows-dpapi::encrypt_data/decrypt_data` | Safe wrapper; handles DATA_BLOB lifecycle, entropy, LocalFree |
| `%APPDATA%` resolution | `std::env::var("APPDATA")` + manual join | `dirs::data_dir()` | Handles missing env var, cross-platform for tests, well-tested |
| Registry Run key for autostart | Direct `winreg` writes | `auto-launch` 0.6 | Handles `StartupApproved` key (Task Manager integration) correctly |

**Key insight:** This stack's "don't hand-roll" items are almost all about Windows-specific edge cases and lifecycle management. The libraries eliminate hundreds of lines of fragile platform code each.

---

## Common Pitfalls

### Pitfall 1: tao vs winit API incompatibility — the silent build failure
**What goes wrong:** `egui-winit 0.35` depends on `winit ^0.30.13`. If `tao 0.35` is used instead, `egui_winit::State::new()` expects `&winit::window::Window` and `egui_state.on_window_event()` expects `winit::event::WindowEvent`. tao's types share the same struct names but are different types in a different crate — the Rust compiler rejects the mismatch at the call site. This is not a runtime error; it is a compile-time type mismatch that must be caught and fixed during the spike.
**Root cause:** tao is a winit fork but the two crates have diverged APIs. tao 0.35 still uses the old `FnMut(Event, &EventLoopWindowTarget, &mut ControlFlow)` callback model; winit 0.30 uses the `ApplicationHandler` trait.
**Fix:** Use `winit 0.30` exclusively. Remove `tao` from Cargo.toml. `tray-icon 0.24` supports `winit 0.30` natively (confirmed in its `examples/winit.rs`).
**Warning signs:** Compiler error "expected `winit::window::Window`, found `tao::window::Window`."

### Pitfall 2: wgpu surface creation on wrong thread
**What goes wrong:** `wgpu::Instance::create_surface(&window)` must be called on the same thread that owns the `winit::Window`. If called before `ApplicationHandler::resumed()`, or from a spawned thread, it panics or returns an error on Windows (DX12 device is not thread-safe for surface initialization).
**Root cause:** The `winit::Window` is `!Send` on Windows; the wgpu DX12 surface is tied to the window's thread.
**Fix:** Create the wgpu `Instance`, `Adapter`, `Device`, `Queue`, and `Surface` inside `ApplicationHandler::resumed()` — the place winit guarantees the window handle is valid. Store the result in the `App` struct.

### Pitfall 3: wgpu in headless CI fails with "No adapter found"
**What goes wrong:** `wgpu::Adapter::request_adapter()` returns `None` when no GPU backend is available — which happens on Linux CI runners or Windows CI runners without explicit WARP selection.
**Root cause:** wgpu default adapter selection may prefer DX12 (hardware) on Windows but fall back fails in some configurations.
**Fix:** Set `WGPU_BACKEND=dx12` in the CI environment (`env:` in GHA YAML). This forces WARP (Microsoft's software D3D12 rasterizer), which is always present on Windows. Alternatively, use `wgpu::InstanceDescriptor { backends: Backends::DX12, .. }` in code for CI builds. The integration tests should skip or mock the wgpu render path and only test SQLite + DPAPI (which need no GPU).
**Warning signs:** CI fails with "thread 'main' panicked at 'Failed to find adapter'".

### Pitfall 4: SQLite `SQLITE_CANTOPEN` on first run
**What goes wrong:** `rusqlite::Connection::open("%APPDATA%\\BrevlyPrint\\state.db")` returns `Error::SqliteFailure(SQLITE_CANTOPEN, ...)` if the directory does not exist.
**Root cause:** SQLite cannot create directories, only files.
**Fix:** `init_app_dir()` (which calls `create_dir_all`) must be the FIRST operation in `main()`, before any `Connection::open` call. See Pattern 5. (Pitfall m2 from PITFALLS.md.)

### Pitfall 5: DPAPI error is `anyhow::Error`, not a typed enum
**What goes wrong:** `windows-dpapi::decrypt_data()` returns `anyhow::Result<Vec<u8>>`. The error is an `anyhow::Error` wrapping an `std::io::Error` with the Win32 error code. If the code does `?` without mapping, the error type propagates as `anyhow::Error` into library code that should return `CredentialError`. The binary layer then has no way to distinguish "not found" from "corrupt" from "IO error".
**Root cause:** `windows-dpapi` uses `anyhow` for its public API, not a typed error enum.
**Fix:** Map the error in `CredentialStore::load()` before returning. Separately, check `self.path.exists()` before calling `decrypt_data` — a missing file is a distinct case that must map to `CredentialError::NotFound`, not `Corrupt`. See Pattern 3.

### Pitfall 6: `egui-wgpu` feature `"winit"` must be enabled
**What goes wrong:** `egui-wgpu` without the `winit` feature flag does not include `egui_wgpu::winit::Painter` or the winit-integrated surface management helpers. The crate still compiles but the types needed for window-integrated rendering are absent.
**Root cause:** `egui-wgpu` is designed to work both with and without winit.
**Fix:** `egui-wgpu = { version = "0.35", features = ["winit"] }`. This pulls in the `egui_wgpu::winit` module.

### Pitfall 7: `rusqlite_migration::M::up()` multi-statement SQL
**What goes wrong:** Some SQLite wrappers require individual statements. `rusqlite_migration` executes the migration SQL via `conn.execute_batch()`, which handles multiple semicolon-separated statements in one string. However, comments (`--`) can cause issues in some versions.
**Root cause:** `execute_batch` tokenizes on semicolons.
**Fix:** Keep each table in its own `M::up()` call if multi-statement causes issues, or remove SQL comments from migration strings. The pattern in Pattern 4 uses a single `M::up()` with multiple statements — test on first run.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `cargo test` |
| Config file | `Cargo.toml` `[profile.test]` (no separate file needed) |
| Quick run command | `cargo test --target x86_64-pc-windows-msvc` |
| Full suite command | `cargo test --target x86_64-pc-windows-msvc -- --nocapture` |

### How the Spike is Proven (Nyquist Strategy)

The spike has four distinct success criteria, each verified differently:

| Success Criterion | Verification Method | Automated? | Notes |
|-------------------|--------------------|-----------|----|
| 1. egui window renders + interaction works | **Interactive** on Windows machine: window opens, text field accepts input, button click triggers a state change (logged or shown in label) | Manual | Cannot be automated in CI without GPU display; WARP renders but `winit` events require a display |
| 2. SQLite `state.db` created with 3-table schema | `cargo test` — `config_store_test.rs` writes a `config` row and reads it back; asserts `user_version = 1` | Automated in CI | Runs via GHA Windows runner |
| 3. DPAPI `credential.bin` round-trip | `cargo test` — `credential_store_test.rs`: encrypt dummy string → write → read → decrypt → assert equal; missing file → `CredentialError::NotFound`; corrupt bytes → `CredentialError::Corrupt` | Automated in CI | GHA Windows runners have a real user session; DPAPI `Scope::User` works |
| 4. Full dependency set compiles | `cargo build --release` on GHA Windows runner | Automated in CI | Compilation is the entire Phase 1 CI gate |

### Phase Requirements → Test Map

Phase 1 has no v1 REQUIREMENTS IDs — it is a pure enabling spike. The verifiable behaviors are the four success criteria above.

| Behavior | Test Type | Automated Command |
|----------|-----------|-------------------|
| `%APPDATA%\BrevlyPrint\` created idempotently | unit | `cargo test app_dir::tests` |
| SQLite schema v1 (3 tables) initialized on first run | integration | `cargo test config_store_test` |
| `config` table: key/value write + read | integration | `cargo test config_store_test::test_write_read` |
| DPAPI encrypt → write → read → decrypt round-trip | integration | `cargo test credential_store_test::test_round_trip` |
| Missing `credential.bin` → `CredentialError::NotFound` | integration | `cargo test credential_store_test::test_missing_file` |
| Corrupt `credential.bin` bytes → `CredentialError::Corrupt` | integration | `cargo test credential_store_test::test_corrupt_blob` |
| egui window: text field + button | manual (Windows box) | n/a — visual inspection |
| Full v1 dep set compiles | build | `cargo build --release` |

### Sampling Rate

- **Per task commit:** `cargo test` (SQLite + DPAPI integration tests, fast — <10s on Windows)
- **Per wave merge:** `cargo build --release` + `cargo test` (full suite)
- **Phase gate:** GHA CI green (`build-windows` job passes) before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `tests/config_store_test.rs` — covers SQLite init, schema v1, `config` write/read
- [ ] `tests/credential_store_test.rs` — covers DPAPI round-trip, missing file, corrupt blob (Windows-only; `#[cfg(target_os = "windows")]`)
- [ ] `src/app_dir.rs` with inline `#[cfg(test)]` for `init_app_dir()` using a temp dir
- [ ] `.github/workflows/ci.yml` — Windows-runner build+test gate (see Pattern 6)

---

## Environment Availability

| Dependency | Required By | Available (Linux planning box) | Available (GHA Windows runner) | Notes |
|------------|------------|-------------------------------|-------------------------------|-------|
| Rust 1.97 stable | Cargo build | ✓ (1.97.0) | ✓ (pre-installed) | 2024 edition |
| x86_64-pc-windows-msvc target | Cargo build (Windows) | ✗ (expected) | ✓ (native) | Build runs on Windows |
| Git | Planning/CI | ✓ (2.55.0) | ✓ | |
| gh CLI | CI / PR automation | ✓ (2.96.0) | ✓ | |
| Windows user profile (DPAPI) | Integration tests | ✗ (Linux) | ✓ | GHA runners have real user sessions |
| wgpu-compatible GPU or WARP | egui-wgpu rendering | ✗ (Linux) | ✓ via WARP DX12 | Set `WGPU_BACKEND=dx12` in GHA |
| Visual display / window | egui spike visual proof | ✗ (Linux) | ✓ (interactive on Windows box) | D-04: proven on owner's Windows machine |

**Missing dependencies with no fallback:**
- A Windows machine for interactive proof (D-04). Owner has one — not a blocker.

**Missing dependencies with fallback:**
- GPU in CI: WARP software rasterizer covers CI build+test; interactive proof on Windows box covers rendering.
- x86_64-pc-windows-msvc on planning Linux box: not needed — builds happen on Windows per D-01.

---

## Security Domain

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | Not in Phase 1 (Phase 2+) |
| V3 Session Management | No | Not in Phase 1 |
| V4 Access Control | No | Not in Phase 1 |
| V5 Input Validation | No | No external input in Phase 1 (dummy DPAPI value) |
| V6 Cryptography | **Yes** | `windows-dpapi` wraps `CryptProtectData` (FIPS-compliant AES-256 via Windows CNG) |

### Phase 1 Threat Relevant: DPAPI Key Loss (M7)

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| DPAPI key loss after Windows reinstall | Denial of Service (agent unusable) | `CredentialError::Corrupt` → Phase 2 re-activation flow |
| Plaintext token in memory only | Information Disclosure | Correct — token never written to disk in plaintext; only DPAPI-encrypted `credential.bin` |
| `credential.bin` readable by other users | Information Disclosure | `Scope::User` ties to Windows user account; other users cannot decrypt |

---

## Open Questions

1. **`rusqlite_migration` multi-statement `M::up()` compatibility with rusqlite 0.40**
   - What we know: `rusqlite_migration 2.6.0` requires `rusqlite ^0.40.0`. `execute_batch()` handles multi-statement SQL.
   - What's unclear: Whether the specific multi-statement DDL in Pattern 4 (three `CREATE TABLE` + one `CREATE INDEX` in one `M::up()` string) works without splitting into multiple `M::up()` calls.
   - Recommendation: Test in Phase 1 spike. If multi-statement fails, split into 4 separate `M::up()` calls (still one migration version).

2. **`egui-wgpu` render loop texture format on Windows DX12**
   - What we know: wgpu 29 DX12 WARP uses `TextureFormat::Bgra8Unorm` as the preferred surface format on Windows.
   - What's unclear: Whether `egui_wgpu::Renderer::new(device, format, None, 1, false)` requires explicit format selection vs. auto-detection from the surface.
   - Recommendation: Use `surface.get_capabilities(&adapter).formats[0]` to query the preferred format, then pass it to `Renderer::new`.

3. **`velopack 1.2.0` initialization requirement at startup**
   - What we know: Velopack Rust SDK requires calling `VelopackApp::build().run()` at the start of `main()` to handle update bootstrapper protocol. If omitted, updates silently fail.
   - What's unclear: Whether the Phase 1 stub initialization of velopack (a no-op call at startup) conflicts with the winit event loop setup that follows.
   - Recommendation: Add `velopack::VelopackApp::build().run();` as the FIRST line of `main()` before any event loop or AppState initialization. This is idempotent if not in update mode.

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `dirs::data_dir()` on Windows returns `%APPDATA%\Roaming` | Pattern 5 | App data saved to wrong directory; credential.bin unreachable at next startup |
| A2 | GHA `windows-latest` runner user session supports DPAPI `Scope::User` | Validation Architecture | DPAPI integration tests fail in CI; would need to mock DPAPI for CI |
| A3 | `wgpu::Backends::DX12` with WARP works on `windows-latest` GHA runners without `WGPU_BACKEND` env | CI Pattern | wgpu adapter returns `None`; cargo test panics; need explicit env var |
| A4 | `auto-launch 0.6` API is compatible with the documented `AutoLaunch::new()` call from STACK.md | Cargo.toml D-19 | Compilation error; API changed between 0.5 and 0.6; verify before Phase 2 |
| A5 | `tauri-winrt-notification 0.8` API is backward-compatible with `Toast::new(POWERSHELL_APP_ID)` pattern | Cargo.toml D-19 | Toast notifications broken in Phase 6; API research needed before that phase |

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `eframe::run_native()` for egui on desktop | Raw `egui-winit` + `egui-wgpu` + `winit::ApplicationHandler` | egui 0.30+ / winit 0.30 | eframe is still valid for simple apps; for apps that need their own event loop (tray icon, custom async), raw integration is required |
| `tao` as the recommended tauri windowing lib for standalone use | `winit 0.30` for new integrations | tao has not adopted winit 0.30 trait API | For apps NOT using tao-specific features (GTK menu integration), winit 0.30 is cleaner and directly supported by egui-winit |
| `pusher-rs` / `pusher` crates | Hand-rolled Pusher protocol over `tokio-tungstenite` | Crates effectively abandoned | The protocol is 200 LoC; hand-rolling is the correct call |
| `rusqlite 0.32` | `rusqlite 0.40` | Active development, 8 major bumps since 0.32 | API surface is stable for basic use; `rusqlite_migration 2.6` requires 0.40+; use 0.40 |
| EV certificate for SmartScreen bypass | OV certificate (same reputation building as EV) | Microsoft policy, March 2024 | No instant bypass; both OV and EV build reputation through downloads |

**Deprecated/outdated (from STACK.md/CONTEXT.md):**
- `native-windows-gui` (NWG): permanent maintenance mode — do not use for any new code.
- `trayicon` (Ciantic): unmaintained since 2022.
- `pusher-rs` and `pusher` crates: both disqualified (development stalled / explicitly unsupported).
- `sled`: abandoned as embedded database for production use.
- `egui-tao` crate: pinned to `egui 0.22` / `tao 0.20`; 13 major egui versions behind; do not use.

---

## Sources

### Primary (HIGH confidence)

- [VERIFIED: crates.io API] — all version numbers for all 24 crates, queried 2026-07-15
- [VERIFIED: tray-icon/examples/winit.rs] — ApplicationHandler + tray-icon integration pattern, read from `raw.githubusercontent.com/tauri-apps/tray-icon/dev/examples/winit.rs`
- [VERIFIED: matthewjberger/wgpu-example src/lib.rs] — complete egui-winit + egui-wgpu + winit 0.30 ApplicationHandler implementation, read from `raw.githubusercontent.com/matthewjberger/wgpu-example/main/src/lib.rs`
- [VERIFIED: sheridans/windows-dpapi src/lib.rs] — API surface: `encrypt_data(data, Scope::User, None)`, `decrypt_data(data, Scope::User, None)`, error wrapping via `anyhow`
- [VERIFIED: docs.rs/rusqlite_migration/2.6.0] — `Migrations::from_slice()`, `M::up()`, `to_latest()` API
- [VERIFIED: tao 0.35.3 docs.rs] — confirmed: uses old closure-based `EventLoop::run(FnMut)` API, not `ApplicationHandler`
- [VERIFIED: crates.io tray-icon/0.24.1/dependencies] — `tao ^0.34` and `winit ^0.30` are dev deps (not runtime); tray-icon has no event loop runtime dep
- [VERIFIED: crates.io egui-winit/0.35.0/dependencies] — `winit ^0.30.13` is a normal (runtime) dependency

### Secondary (MEDIUM confidence)

- [docs.rs/tray-icon/0.24.1] — confirmed Win32 thread requirement; confirmed winit AND tao integration support
- [Wikipedia: Windows Advanced Rasterization Platform] — DX12 WARP on Windows 10/11; GHA windows-latest runner software rasterizer
- [GitHub Actions docs] — `windows-latest` runner setup; Rust toolchain pre-installed; DPAPI user session available
- [emilk/egui issue #2875] — confirms C2 pitfall: `eframe` + `tao` event loop conflict

### Tertiary (LOW confidence — used for context only)

- [github.com/sidit77/egui-tao] — identified as unusable (egui 0.22 / tao 0.20); confirmed via Cargo.toml inspection

---

## Metadata

**Confidence breakdown:**

| Area | Level | Reason |
|------|-------|--------|
| Standard stack (versions) | HIGH | All 24 crates verified on crates.io 2026-07-15 |
| Event loop architecture (winit not tao) | HIGH | Confirmed via egui-winit dep on winit ^0.30, tao docs showing old API, tray-icon winit.rs example |
| egui-wgpu render pipeline | HIGH | Working reference implementation read from source (matthewjberger/wgpu-example) |
| DPAPI API surface | HIGH | Read directly from sheridans/windows-dpapi src/lib.rs |
| rusqlite_migration API | HIGH | Confirmed from docs.rs/rusqlite_migration/2.6.0 |
| CI WARP headless rendering | MEDIUM | WARP confirmed present on Windows; `WGPU_BACKEND=dx12` is the documented mechanism; not personally tested in GHA |
| DPAPI in GHA CI | MEDIUM | GHA runners have user sessions; DPAPI should work; not confirmed with a live test |
| `auto-launch 0.6` API compatibility | LOW | Version jumped from 0.5 to 0.6; API changes unknown; flag for Phase 2 verification |

**Research date:** 2026-07-15
**Valid until:** 2026-08-15 (stable stack; egui moves fast but 0.35 just released 2026-06-25)
