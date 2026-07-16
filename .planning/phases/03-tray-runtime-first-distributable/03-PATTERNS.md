# Phase 3: Tray + Runtime + First Distributable - Pattern Map

**Mapped:** 2026-07-16
**Files analyzed:** 7 (new/modified files for Phase 3)
**Analogs found:** 7 / 7

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `src/main.rs` (modify) | entrypoint/orchestrator | request-response + event-driven | `src/main.rs` itself (extend) | exact — same file |
| `src/health_state.rs` (new) | model/enum | transform (state → string/icon) | `src/activation_state.rs` (`FlowState` enum + label mapping) | role-match |
| `src/tray_runtime.rs` (new) | Windows-only service | event-driven | `src/printer/spooler.rs` (`#[cfg(windows)]` + Win32 FFI pattern) | role-match |
| `src/assets/tray_green.rgba` (new) | static asset | n/a | `src/printer/spooler.rs` (PCWSTR WR-06 UTF-16 byte construction pattern) | reference only |
| `src/assets/tray_yellow.rgba` (new) | static asset | n/a | same | reference only |
| `src/assets/tray_red.rgba` (new) | static asset | n/a | same | reference only |
| `.github/workflows/ci.yml` (modify) | CI config | batch/build | `.github/workflows/ci.yml` itself (extend Windows job) | exact — same file |

---

## Pattern Assignments

---

### `src/main.rs` (entrypoint — restructure existing)

**Analog:** `src/main.rs` (existing Phase 2 file — extend in place)

**What changes:** The `UserEvent` enum gains three variants. The `App` struct gains `mode`
(Activation vs Runtime), `health`, `tray`, and `menu_items` fields. The early-exit stub at
`needs_activation == false` is replaced with Runtime mode. A single-instance mutex check is
inserted after the Velopack bootstrapper. Event proxy wiring is moved before `run_app()`.

**Existing imports block** (`src/main.rs` lines 12–24 — extend these, do not replace):
```rust
use anyhow::Context as _;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ControlFlow, EventLoop},
};
use brevly_print::{
    activation_window::ActivationWindow,
    app_dir::init_app_dir,
    config_store,
    credential_store::{credential_store, CredentialError, CredentialStore as _},
};
```

**Add for Phase 3** (Windows-only imports block pattern — mirror `src/printer/spooler.rs` lines 10–18):
```rust
#[cfg(windows)]
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
    System::Threading::CreateMutexW,
    UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, IDYES,
    },
};
#[cfg(windows)]
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};
#[cfg(windows)]
use tray_icon::menu::{Menu, MenuItem, MenuEvent, PredefinedMenuItem};
use brevly_print::health_state::HealthState;
#[cfg(windows)]
use brevly_print::tray_runtime::{TrayRuntime, TrayMenuItems};
```

**Existing `UserEvent` enum** (`src/main.rs` lines 32–35) — replace the stub with:
```rust
#[derive(Debug)]
enum UserEvent {
    TrayIconEvent(tray_icon::TrayIconEvent),
    MenuEvent(tray_icon::menu::MenuEvent),
    HealthChanged(HealthState),
}
```

**Existing `App` struct** (`src/main.rs` lines 40–53) — extend with mode fields:
```rust
struct App {
    // === Phase 2 fields (keep) ===
    rt: tokio::runtime::Handle,
    http: reqwest::Client,
    app_dir: std::path::PathBuf,
    conn: rusqlite::Connection,

    // === Phase 3 additions ===
    /// AppMode: which startup path we are in.
    mode: AppMode,
    /// Current health state (Phase 3 seeds Connected; Phase 4 drives transitions).
    health: HealthState,
    /// Tray runtime (Windows-only, None in Activation mode and on Linux).
    #[cfg(windows)]
    tray_runtime: Option<TrayRuntime>,
    /// Activation window (Some only when Activation mode or on-demand Reativar).
    activation_window: Option<ActivationWindow>,
    /// is_reactivation flag for ActivationWindow constructor (Phase 2 field, renamed).
    is_reactivation: bool,
}

enum AppMode {
    Activation,
    Runtime,
}
```

**Existing `new_events` stub** — Phase 2 has no `new_events`; Phase 3 adds it:
```rust
fn new_events(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, cause: winit::event::StartCause) {
    #[cfg(windows)]
    if cause == winit::event::StartCause::Init {
        if matches!(self.mode, AppMode::Runtime) {
            // CRITICAL: tray creation must happen here, not before run_app().
            // See RESEARCH.md Pattern 1 — Win32 message pump must be running.
            match TrayRuntime::new(self.health) {
                Ok(rt) => self.tray_runtime = Some(rt),
                Err(e) => {
                    eprintln!("[brevly-print] Failed to create tray icon: {e:#}");
                    event_loop.exit();
                }
            }
        }
    }
}
```

**Existing `resumed` handler** (`src/main.rs` lines 56–74) — add mode branch:
```rust
fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
    match self.mode {
        AppMode::Activation => {
            // Existing Phase 2 code — unchanged
            if self.activation_window.is_none() {
                match ActivationWindow::new(
                    event_loop, self.rt.clone(), self.http.clone(),
                    self.is_reactivation, self.app_dir.clone(),
                ) {
                    Ok(w) => self.activation_window = Some(w),
                    Err(e) => {
                        eprintln!("[brevly-print] Failed to create activation window: {e:#}");
                        event_loop.exit();
                    }
                }
            }
        }
        AppMode::Runtime => {
            // No window to create — tray is created in new_events(Init).
        }
    }
}
```

**Existing `user_event` handler** (`src/main.rs` lines 120–128) — replace empty match:
```rust
fn user_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: UserEvent) {
    match event {
        UserEvent::TrayIconEvent(_e) => {
            // D-07: left-click is no-op in Phase 3
        }
        UserEvent::MenuEvent(e) => {
            #[cfg(windows)]
            self.handle_menu_event(event_loop, e);
        }
        UserEvent::HealthChanged(state) => {
            self.health = state;
            #[cfg(windows)]
            if let Some(rt) = &self.tray_runtime {
                rt.apply_health(state);
            }
        }
    }
}
```

**Existing `about_to_wait` handler** (`src/main.rs` lines 130–136) — add Runtime branch:
```rust
fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
    match self.mode {
        AppMode::Activation => {
            // Existing Phase 2: request redraw for egui animation
            if let Some(window) = self.activation_window.as_ref() {
                window.window().request_redraw();
            }
        }
        AppMode::Runtime => {
            // ControlFlow::Wait already set; no redraw loop needed.
            // The runtime is idle until a tray/menu/health event arrives.
        }
    }
}
```

**Single-instance mutex insertion point** (`src/main.rs` line 145, after Velopack bootstrapper):
```rust
// After VelopackApp::build().run() — before building tokio runtime.
// D-08/D-09: named mutex guard; second instance exits silently.
#[cfg(windows)]
let _mutex_guard = {
    use std::iter::once;
    let name: Vec<u16> = "Local\\BrevlyPrintAgent"
        .encode_utf16().chain(once(0)).collect();
    let result = unsafe {
        CreateMutexW(None, false, windows::core::PCWSTR(name.as_ptr()))
    };
    match result {
        Ok(handle) => {
            let last_err = unsafe { GetLastError() };
            if last_err == ERROR_ALREADY_EXISTS {
                let _ = unsafe { CloseHandle(handle) };
                return Ok(()); // silent exit — another instance is running
            }
            handle // hold for process lifetime
        }
        Err(_) => return Ok(()), // conservative: mutex failure → exit
    }
};
```

**Event proxy wiring** (`src/main.rs` line 210 comment "Phase 3: wire..."):
```rust
// Wire tray + menu event forwarding into the winit event loop BEFORE run_app().
// Two separate proxies because each closure captures its own clone.
// Pattern: src/main.rs user_event handler receives these as UserEvent variants.
#[cfg(windows)]
{
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::TrayIconEvent(event));
    }));

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::MenuEvent(event));
    }));
}
```

**Early-exit stub replacement** (`src/main.rs` lines 199–202):
```rust
// Replace:
//   if !needs_activation { println!(...); return Ok(()); }
// With mode-based App construction (no early exit):
let mode = if needs_activation { AppMode::Activation } else { AppMode::Runtime };
let health = HealthState::Connected; // D-02: seed Connected on successful startup
```

---

### `src/health_state.rs` (new — portable enum + Windows icon accessor)

**Analog:** `src/activation_state.rs` — same pattern: plain enum + associated methods,
portable core with a `#[cfg(windows)]` extension block for platform-specific accessor.

**Module doc header pattern** (copy from `src/activation_state.rs` line 1–6 style):
```rust
//! Health state machine for the tray agent.
//!
//! Portable — no `#[cfg(windows)]` on the enum or string mappings.
//! The `#[cfg(windows)]` block adds the `icon()` accessor used by `tray_runtime.rs`.
```

**Enum definition pattern** (matches `FlowState` in `src/activation_state.rs` lines 16–29):
```rust
/// Tri-color connection state reflected in the tray icon (RUN-02).
///
/// Phase 3 seeds `Connected`. Phase 4 (Pusher) drives `Reconnecting`/`Connected`.
/// Phase 6 (printer failure) drives `Problem`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    /// Green: Pusher connected, printer reachable. Seeded at startup (D-02).
    Connected,
    /// Yellow: WebSocket handshake in progress or reconnect backoff. Phase 4+.
    Reconnecting,
    /// Red: Printer absent or connection failed. D-03 / Phase 6.
    Problem,
}
```

**String label methods** (same style as `FlowState` match arms in `activation_state.rs`):
```rust
impl HealthState {
    /// Tooltip shown on hover over the tray icon.
    pub fn tooltip(&self) -> &'static str {
        match self {
            Self::Connected    => "Brevly Print — Conectado",
            Self::Reconnecting => "Brevly Print — Reconectando…",
            Self::Problem      => "Brevly Print — Problema de conexão",
        }
    }

    /// PT-BR label for the disabled status menu item (D-06).
    pub fn status_label(&self) -> &'static str {
        match self {
            Self::Connected    => "Conectado",
            Self::Reconnecting => "Reconectando…",
            Self::Problem      => "Problema de conexão",
        }
    }
}
```

**Windows-only `icon()` accessor** (pattern: `#[cfg(windows)]` impl block, same as
`src/machine_id.rs` lines 21–31 and `src/printer/mod.rs` lines 100–131):
```rust
#[cfg(windows)]
impl HealthState {
    /// Load the corresponding 16×16 RGBA tray icon.
    ///
    /// Assets are embedded at compile time via `include_bytes!` (D-05).
    /// Each file is exactly 16 × 16 × 4 = 1024 bytes of raw RGBA.
    pub fn icon(&self) -> tray_icon::Icon {
        let (bytes, w, h): (&[u8], u32, u32) = match self {
            Self::Connected    => (include_bytes!("../assets/tray_green.rgba"),  16, 16),
            Self::Reconnecting => (include_bytes!("../assets/tray_yellow.rgba"), 16, 16),
            Self::Problem      => (include_bytes!("../assets/tray_red.rgba"),    16, 16),
        };
        tray_icon::Icon::from_rgba(bytes.to_vec(), w, h)
            .expect("embedded tray RGBA bytes are always valid")
    }
}
```

**Unit test block pattern** (mirrors existing test blocks in the codebase — portable, runs on Linux):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_states_have_distinct_tooltips() {
        let tooltips: Vec<_> = [
            HealthState::Connected,
            HealthState::Reconnecting,
            HealthState::Problem,
        ]
        .iter()
        .map(|s| s.tooltip())
        .collect();
        // All distinct
        assert_eq!(tooltips.len(), 3);
        assert_ne!(tooltips[0], tooltips[1]);
        assert_ne!(tooltips[1], tooltips[2]);
        assert_ne!(tooltips[0], tooltips[2]);
    }

    #[test]
    fn status_labels_are_non_empty() {
        for state in [HealthState::Connected, HealthState::Reconnecting, HealthState::Problem] {
            assert!(!state.status_label().is_empty());
        }
    }
}
```

---

### `src/tray_runtime.rs` (new — Windows-only tray creation + menu)

**Analog:** `src/printer/spooler.rs` — Windows-only (`#[cfg(windows)]` at file top via `#![cfg(windows)]`),
holds Win32 FFI logic, uses the PCWSTR UTF-16 null-termination pattern, separates struct from impl.

**File-level cfg gate** (copy exactly from `src/printer/spooler.rs` line 10):
```rust
#![cfg(windows)]
```

**Module doc header** (same style as spooler.rs lines 1–9):
```rust
//! Windows tray icon runtime — creation, menu, and health-state updates.
//!
//! **Windows-only.** Compiled only when `cfg(windows)`.
//!
//! Encapsulates all `tray-icon` + `muda` interaction: creates the `TrayIcon` in
//! `new_events(StartCause::Init)`, builds the right-click menu, and applies
//! `HealthState` changes by swapping icon + tooltip + status label.
//!
//! CRITICAL: `TrayRuntime::new()` must be called ONLY from `ApplicationHandler::new_events()`
//! when `cause == StartCause::Init` — the Win32 message pump must be running first.
```

**Imports block** (mirrors spooler.rs lines 12–19 — use `windows::` crate for Win32):
```rust
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, IDYES,
    MESSAGEBOX_RESULT,
};
use windows::core::PCWSTR;

use tray_icon::{TrayIcon, TrayIconBuilder};
use tray_icon::menu::{Menu, MenuItem, MenuEvent, PredefinedMenuItem};
use winit::event_loop::ActiveEventLoop;

use crate::health_state::HealthState;
```

**Struct definition** (same shape as `WindowsSpoolerPrinter` in spooler.rs lines 24–28):
```rust
/// Holds the live `TrayIcon` and menu item handles.
///
/// Constructed once in `new_events(StartCause::Init)`; held in `App` for process lifetime.
pub struct TrayRuntime {
    tray: TrayIcon,
    pub menu_items: TrayMenuItems,
}

/// Handles for the four right-click menu items (D-06).
pub struct TrayMenuItems {
    pub status:   MenuItem,   // disabled status line
    pub reativar: MenuItem,
    pub sobre:    MenuItem,
    pub sair:     MenuItem,
}
```

**Constructor** (same shape as `WindowsSpoolerPrinter::new` in spooler.rs lines 31–33):
```rust
impl TrayRuntime {
    /// Create the tray icon and right-click menu.
    ///
    /// Must be called from `ApplicationHandler::new_events(StartCause::Init)`.
    pub fn new(health: HealthState) -> anyhow::Result<Self> {
        let (menu, menu_items) = build_tray_menu(health);
        let tray = TrayIconBuilder::new()
            .with_icon(health.icon())
            .with_tooltip(health.tooltip())
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false) // D-07: left-click is no-op
            .build()
            .map_err(|e| anyhow::anyhow!("TrayIconBuilder::build failed: {e}"))?;
        Ok(Self { tray, menu_items })
    }

    /// Swap icon, tooltip, and status label to reflect a new health state.
    ///
    /// Called from `App::user_event(UserEvent::HealthChanged(_))` on the event-loop thread.
    pub fn apply_health(&self, health: HealthState) {
        let _ = self.tray.set_icon(Some(health.icon()));
        let _ = self.tray.set_tooltip(Some(health.tooltip()));
        self.menu_items.status.set_text(health.status_label());
    }

    /// Expose tray menu item IDs for menu event dispatch in `App`.
    pub fn menu_items(&self) -> &TrayMenuItems {
        &self.menu_items
    }
}
```

**PCWSTR helper** — copy the WR-06 pattern from `src/printer/spooler.rs` lines 54–58 verbatim:
```rust
/// Build a null-terminated UTF-16 wide string for Win32 PCWSTR parameters.
///
/// WR-06: established pattern from src/printer/spooler.rs — use `.chain(std::iter::once(0))`.
fn to_wstr(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
```

**MessageBoxW helpers** (same unsafe block pattern as spooler.rs `write_raw_to_spooler`):
```rust
/// Show the "Sobre" info dialog (D-06).
pub fn show_about_dialog() {
    let version = env!("CARGO_PKG_VERSION");
    let text    = to_wstr(&format!("Brevly Print v{}\n\nAgente de impressão para Noren.", version));
    let caption = to_wstr("Sobre o Brevly Print");
    unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(caption.as_ptr()), MB_OK | MB_ICONINFORMATION);
    }
}

/// Show the "Sair" confirmation dialog (D-06). Returns true if user confirmed quit.
pub fn confirm_quit_dialog() -> bool {
    let text = to_wstr(
        "Fechar o Brevly Print?\nAs impressões vão parar enquanto o programa estiver fechado."
    );
    let caption = to_wstr("Brevly Print — Sair");
    let result: MESSAGEBOX_RESULT = unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(caption.as_ptr()), MB_YESNO | MB_ICONQUESTION)
    };
    result == IDYES
}
```

**Menu construction** (function extracted from struct — same separation as spooler.rs `submit_job`):
```rust
fn build_tray_menu(health: HealthState) -> (Menu, TrayMenuItems) {
    let status   = MenuItem::new(health.status_label(), false, None); // false = disabled
    let reativar = MenuItem::new("Reativar impressora/licença", true, None);
    let sobre    = MenuItem::new("Sobre", true, None);
    let sair     = MenuItem::new("Sair", true, None);

    let menu = Menu::new();
    let _ = menu.append(&status);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&reativar);
    let _ = menu.append(&sobre);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&sair);

    (menu, TrayMenuItems { status, reativar, sobre, sair })
}
```

---

### `src/assets/tray_green.rgba`, `tray_yellow.rgba`, `tray_red.rgba` (new static assets)

**No analog in the codebase** — these are raw binary files (1024 bytes each: 16×16 RGBA).

**Generation pattern** (from RESEARCH.md Pattern 3 — implement in `build.rs` or a one-time script):
```rust
// build.rs — generates solid-color 16×16 RGBA files at build time
fn write_solid_rgba(out_dir: &str, filename: &str, r: u8, g: u8, b: u8) {
    let path = std::path::Path::new(out_dir).join(filename);
    let mut bytes = Vec::with_capacity(16 * 16 * 4);
    for _ in 0..(16 * 16) {
        bytes.extend_from_slice(&[r, g, b, 255]);
    }
    std::fs::write(path, &bytes).unwrap();
}
// Colors (D-05): green=#22C55E, yellow=#F59E0B, red=#EF4444
// write_solid_rgba(out_dir, "tray_green.rgba",  0x22, 0xC5, 0x5E);
// write_solid_rgba(out_dir, "tray_yellow.rgba", 0xF5, 0x9E, 0x0B);
// write_solid_rgba(out_dir, "tray_red.rgba",    0xEF, 0x44, 0x44);
```

**Note:** If committed as static files (simplest), place them in `src/assets/` and reference
via `include_bytes!("../assets/tray_green.rgba")` in `health_state.rs`. If generated via
`build.rs`, output to `OUT_DIR` and reference via `include_bytes!(concat!(env!("OUT_DIR"), "/tray_green.rgba"))`.
Committing the static files is simpler and avoids a `build.rs` dependency — prefer that path
unless `build.rs` already exists for other reasons.

---

### `.github/workflows/ci.yml` (modify — extend Windows job)

**Analog:** `.github/workflows/ci.yml` itself (existing Windows job, lines 47–74)

**Existing Windows job tail** (lines 69–74 — append packaging steps after this):
```yaml
      - name: Build release binary (proves full v1 dep set compiles — SC-4 Windows half)
        run: cargo build --release --target x86_64-pc-windows-msvc

      - name: Run tests (full suite — includes real DPAPI round-trip)
        run: cargo test --target x86_64-pc-windows-msvc
```

**Append these steps to the Windows job** (D-12 pattern from RESEARCH.md Pattern 9):
```yaml
      - name: Install vpk CLI
        run: dotnet tool install -g vpk --version "1.*"

      - name: Package with vpk (produces Setup.exe)
        shell: pwsh
        run: |
          $version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
          vpk pack `
            --packId BrevlyPrint `
            --packVersion $version `
            --packDir target\release `
            --mainExe brevly-print.exe `
            --outputDir Releases `
            --packTitle "Brevly Print" `
            --packAuthors "Brevly"

      # Sign only when OV cert secret is present.
      # Skips cleanly on PRs and before cert procurement (D-12/D-13).
      - name: Sign Setup.exe (Authenticode — OV cert, gated on secret)
        if: ${{ secrets.CODESIGN_PFX_BASE64 != '' }}
        shell: pwsh
        env:
          CODESIGN_PFX_BASE64:  ${{ secrets.CODESIGN_PFX_BASE64 }}
          CODESIGN_PFX_PASSWORD: ${{ secrets.CODESIGN_PFX_PASSWORD }}
        run: |
          $pfxBytes = [Convert]::FromBase64String($env:CODESIGN_PFX_BASE64)
          $pfxPath  = "$env:RUNNER_TEMP\cert.pfx"
          [IO.File]::WriteAllBytes($pfxPath, $pfxBytes)
          $setupExe = Get-ChildItem -Path Releases -Filter "*Setup.exe" | Select-Object -First 1
          & signtool sign /fd SHA256 /f "$pfxPath" /p "$env:CODESIGN_PFX_PASSWORD" `
            /tr http://timestamp.digicert.com /td SHA256 "$($setupExe.FullName)"
          Remove-Item $pfxPath

      - name: Upload Setup.exe artifact
        uses: actions/upload-artifact@v4
        with:
          name: brevly-print-setup
          path: Releases/*Setup.exe
```

---

## Shared Patterns

### 1. `#![cfg(windows)]` file-level gate (Windows-only modules)

**Source:** `src/printer/spooler.rs` line 10; `src/credential_store/dpapi.rs` line 7
**Apply to:** `src/tray_runtime.rs`

The Windows-only module files use `#![cfg(windows)]` at the top (inner attribute, gates the
entire file) rather than wrapping every item in `#[cfg(windows)]`. This is the established
project convention — do not use the outer attribute form for entire-file gates.

```rust
// First line of file, before any `use` or `mod`:
#![cfg(windows)]
```

### 2. Dual `#[cfg(windows)]` / `#[cfg(not(windows))]` factory function

**Source:** `src/credential_store/mod.rs` lines 38–46; `src/printer/mod.rs` lines 59–68 (`enumerate_printers`)
**Apply to:** `src/lib.rs` (exposing `health_state` and `tray_runtime` modules), `src/main.rs`

When a function or module must behave differently on Windows vs Linux, use two separate `#[cfg]`
attributed items rather than an `if cfg!()` inside one function:

```rust
// Pattern from src/credential_store/mod.rs lines 38-46:
#[cfg(windows)]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore { ... }

#[cfg(not(windows))]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore { ... }
```

### 3. WR-06: null-terminated UTF-16 wide string (PCWSTR)

**Source:** `src/printer/spooler.rs` lines 54–58 (name_w) and lines 86–90 (doc_name, datatype)
**Apply to:** `src/tray_runtime.rs` (all `MessageBoxW` calls and the mutex name in `main.rs`)

```rust
// Established in Phase 2 as WR-06 — copy verbatim, do not invent alternatives:
let name_w: Vec<u16> = "Local\\BrevlyPrintAgent"
    .encode_utf16()
    .chain(std::iter::once(0))
    .collect();
// Pass: windows::core::PCWSTR(name_w.as_ptr())
```

### 4. Error propagation style (`PrinterError` / `anyhow`)

**Source:** `src/printer/spooler.rs` lines 62–63 (`.map_err(|e| PrinterError::NotFound(...))`);
`src/main.rs` lines 152–155 (`.context("...")`)
**Apply to:** `src/tray_runtime.rs` constructor errors, `src/main.rs` Runtime mode setup

Windows-only init errors use `anyhow::anyhow!("... {e}")` at the call site:

```rust
.map_err(|e| anyhow::anyhow!("TrayIconBuilder::build failed: {e}"))?;
```

Fatal init errors in `main.rs` use `.context("...")`:

```rust
let runtime = TrayRuntime::new(health).context("Failed to create tray icon")?;
```

### 5. Module declaration + re-export in `src/lib.rs`

**Source:** `src/lib.rs` lines 7–18; `src/printer/mod.rs` lines 10–19
**Apply to:** `src/lib.rs` (add `health_state` and conditional `tray_runtime` module declarations)

```rust
// Pattern from src/lib.rs:
pub mod health_state;

// tray_runtime is Windows-only (cfg gate is inside the file itself via #![cfg(windows)],
// but the module declaration in lib.rs does not need a cfg gate because the file-level
// #![cfg(windows)] prevents the module body from compiling on non-Windows):
pub mod tray_runtime;
```

### 6. `unsafe` block documentation

**Source:** `src/printer/spooler.rs` lines 39–43 (SAFETY comment above `unsafe` block)
**Apply to:** `src/tray_runtime.rs` (MessageBoxW, any raw Win32 calls); `src/main.rs` (CreateMutexW)

```rust
// SAFETY: Win32 FFI — [explain what invariants are upheld].
// Example from spooler.rs:
// SAFETY: Win32 FFI — pointer and length values are correctly derived from owned
// Vec<u16> and slice references that outlive the unsafe block.
unsafe { MessageBoxW(...) };
```

### 7. `ControlFlow::Wait` (already set in Phase 2)

**Source:** `src/main.rs` line 208: `event_loop.set_control_flow(ControlFlow::Wait);`
**Apply to:** Runtime mode in Phase 3 — no change needed; just confirm this line remains and
is not accidentally overridden in the Runtime path. The event loop is already in `Wait` mode.

---

## `Cargo.toml` Change Pattern

**Source:** `Cargo.toml` lines 52–58 (existing `windows` entry)
**Change:** Add two new feature flags to the existing `[target.'cfg(windows)'.dependencies.windows]`
entry (do not create a duplicate entry):

```toml
# Before (Phase 2 state):
windows = { version = "0.62", features = [
    "Win32_Graphics_Printing",
    "Win32_Foundation",
] }

# After (Phase 3 — add two lines):
windows = { version = "0.62", features = [
    "Win32_Graphics_Printing",
    "Win32_Foundation",
    "Win32_System_Threading",       # CreateMutexW, GetLastError (D-08)
    "Win32_UI_WindowsAndMessaging", # MessageBoxW, MB_YESNO, IDYES (D-06)
] }
```

---

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `src/assets/tray_*.rgba` | static binary asset | n/a | No binary asset files exist in the codebase yet; generation pattern is in RESEARCH.md Pattern 3 |

---

## Metadata

**Analog search scope:** `src/` (all 18 files), `.github/workflows/`, `Cargo.toml`
**Files scanned:** 18 source files + CI workflow + Cargo.toml = 20 files
**Pattern extraction date:** 2026-07-16

**Key integration note (RESEARCH.md §Pitfall 2 / Pattern 8):**
The `activation_state.rs` save flow (`auto-launch` HKCU Run registration) must be audited
before Phase 3 ships. If Phase 2 registered `std::env::current_exe()` directly, that path
will be wrong on a Velopack-installed system (points to `current\brevly-print.exe` instead of
the root stub). The fix is in `src/activation_state.rs` save path — detect `Update.exe` in
parent dir and register the grandparent stub path. This is a modification to an existing file
not listed in the new-file inventory, but it is a required correctness fix for RUN-03.
