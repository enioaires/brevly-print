# Phase 2: Activation — Pattern Map

**Mapped:** 2026-07-15
**Files analyzed:** 9 new/modified files
**Analogs found:** 9 / 9 (every new file has a direct codebase analog)

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/activation_window.rs` | component | request-response | `src/spike_window.rs` | exact (same egui-winit+wgpu scaffold, growing the spike UI into the full form) |
| `src/activation_state.rs` | model | event-driven | `src/spike_window.rs` (input/applied state fields) | role-match (same immediate-mode state pattern, expanded) |
| `src/printer/mod.rs` | service + model | request-response | `src/credential_store/mod.rs` | exact (cfg-gated trait + factory function + typed error enum) |
| `src/printer/spooler.rs` | service | request-response | `src/credential_store/dpapi.rs` | exact (Windows-only impl under `#![cfg(windows)]`, struct + trait impl) |
| `src/printer/serial.rs` | service | request-response | `src/credential_store/dpapi.rs` | role-match (Windows-only impl, different hardware path) |
| `src/printer/stub.rs` | service | request-response | `src/credential_store/devfile.rs` | exact (non-Windows stub impl under `#![cfg(not(windows))]`, no-op semantics) |
| `src/noren_client.rs` | service | request-response | `src/credential_store/mod.rs` (typed error enum shape) | partial (new async HTTP code, no codebase analog for HTTP, but error enum mirrors CredentialError) |
| `src/machine_id.rs` | utility | request-response | `src/credential_store/mod.rs` (cfg-gated factory function) | role-match (cfg-gated function, Windows impl + Linux stub) |
| `src/main.rs` | controller | event-driven | `src/main.rs` (existing — modify in place) | exact (ApplicationHandler pattern extended with credential check + tokio runtime) |
| `tests/noren_client_test.rs` | test | request-response | `tests/credential_contract_test.rs` | exact (integration test: trait contract + error variants, using tempfile) |
| `tests/printer_test.rs` | test | request-response | `tests/credential_contract_test.rs` | exact (same integration test pattern: trait contract + Linux stub path) |
| `lib.rs` | config | — | `src/lib.rs` (existing — modify in place) | exact (add new module exports following same pattern) |
| `Cargo.toml` | config | — | `Cargo.toml` (existing — modify in place) | exact (add `winreg` to `[target.'cfg(windows)'.dependencies]`) |

---

## Pattern Assignments

### `src/activation_window.rs` (component, request-response)

**Analog:** `src/spike_window.rs`

This file replaces `spike_window.rs`. The entire scaffold (EguiRenderer inner struct, wgpu init sequence, `draw()` frame loop, `handle_input()` delegation, `resize()`) is copied verbatim and the spike UI closure is replaced with the activation form.

**Struct + field pattern** (`src/spike_window.rs` lines 134–149):
```rust
pub struct SpikeWindow {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    egui_renderer: EguiRenderer,
    // Spike UI state (D-07/D-09)
    input: String,
    applied: String,
}
```
For `ActivationWindow`, replace the two state fields with an `ActivationFormState` (from `activation_state.rs`) and a `tokio::runtime::Handle`.

**Window creation attributes** (`src/spike_window.rs` lines 155–165 — change title/size):
```rust
let attrs = Window::default_attributes()
    .with_title("BrevlyPrint — Spike Window")
    .with_inner_size(winit::dpi::LogicalSize::new(640u32, 480u32))
    .with_visible(false);
```
Phase 2 values per 02-UI-SPEC.md: title `"Brevly Print — Ativação"`, size `440 × 520`, add `.with_resizable(false)`.

**EguiRenderer init** (`src/spike_window.rs` lines 44–61 — copy verbatim):
```rust
let state = egui_winit::State::new(
    context.clone(),
    viewport_id,
    window.as_ref(),
    Some(window.scale_factor() as f32),
    Some(winit::window::Theme::Dark),
    None,
);
let renderer = egui_wgpu::Renderer::new(
    device,
    output_format,
    egui_wgpu::RendererOptions::default(),
);
```

**Tokio runtime for wgpu init** (`src/spike_window.rs` lines 181–183 — adapt):
```rust
let rt = tokio::runtime::Builder::new_current_thread()
    .build()
    .context("Failed to build tokio runtime for wgpu init")?;
```
Phase 2 change: build a `new_multi_thread()` runtime BEFORE the event loop in `main()`, keep it alive for the process lifetime, pass its `Handle` into `ActivationWindow::new()`. The wgpu `block_on` calls move to this same runtime.

**Draw closure pattern** (`src/spike_window.rs` lines 314–338 — replace UI closure body):
```rust
self.egui_renderer.draw(
    &self.device, &self.queue, &mut encoder,
    &self.window, &surface_view, screen_descriptor,
    |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            // Replace spike content with activation form here
        });
    },
);
```

**`draw()` surface texture handling** (`src/spike_window.rs` lines 253–274 — copy verbatim):
```rust
let surface_texture = match self.surface.get_current_texture() {
    wgpu::CurrentSurfaceTexture::Success(t) => t,
    wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
        self.surface.configure(&self.device, &self.surface_config);
        t
    }
    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
        self.surface.configure(&self.device, &self.surface_config);
        return Ok(());
    }
    wgpu::CurrentSurfaceTexture::Timeout
    | wgpu::CurrentSurfaceTexture::Occluded
    | wgpu::CurrentSurfaceTexture::Validation => { return Ok(()); }
};
```

**Clear pass (background color)** (`src/spike_window.rs` lines 288–308 — update r/g/b):
```rust
load: wgpu::LoadOp::Clear(wgpu::Color {
    r: 0.1,   // Phase 2: matches #1A1A1A (26/255 ≈ 0.102)
    g: 0.1,
    b: 0.1,
    a: 1.0,
}),
```
Per 02-UI-SPEC.md palette: background `#1A1A1A` = `r:0.102, g:0.102, b:0.102`. This matches the existing value — no change needed.

**egui visuals setup** (add to `ActivationWindow::new()`, after egui renderer created — from 02-UI-SPEC.md):
```rust
// Set activation window dark theme with brand colors
let mut visuals = egui::Visuals::dark();
visuals.panel_fill          = egui::Color32::from_rgb(26, 26, 26);
visuals.window_fill         = egui::Color32::from_rgb(26, 26, 26);
visuals.faint_bg_color      = egui::Color32::from_rgb(38, 38, 38);
visuals.override_text_color = Some(egui::Color32::from_rgb(245, 245, 245));
egui_renderer.context.set_visuals(visuals);
```

---

### `src/activation_state.rs` (model, event-driven)

**Analog:** `src/spike_window.rs` lines 146–148 (spike state fields) + `src/credential_store/error.rs` (typed error enum shape)

The spike uses two plain `String` fields. Phase 2 promotes this to a dedicated state struct and a flow-state enum, following the thiserror-less enum style used in `CredentialError` (simple variants, no `#[error]` needed for UI-facing enums).

**State fields pattern** (from `src/spike_window.rs` lines 146–148, expanded):
```rust
// In SpikeWindow:
input: String,
applied: String,
```
Expand to a dedicated struct containing all activation form state: serial input, serial error, printer list, selected printer, agent token, flow state, busy flag, async channels, etc.

**Flow state enum pattern** (mirrors `CredentialError` variant style from `src/credential_store/error.rs`):
```rust
// CredentialError shape (src/credential_store/error.rs lines 13–29):
#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("...")]
    NotFound,
    #[error("...")]
    Corrupt(#[source] anyhow::Error),
    #[error("...")]
    Io(#[from] std::io::Error),
}
```
`FlowState` is a plain enum (no `thiserror`, no `#[error]`) since it is UI state, not an error type:
```rust
pub enum FlowState {
    Idle,
    ActivationPending,    // HTTP in flight
    ValidatedAwaitingTestPrint { agent_token: String, tenant_id: String, ... },
    AwaitingTestConfirm,
    ReadyToSave,
    Saving,
}
```

---

### `src/printer/mod.rs` (service + model, request-response)

**Analog:** `src/credential_store/mod.rs` — THE direct pattern source.

**Module layout** (`src/credential_store/mod.rs` lines 1–46 — mirror exactly):
```rust
//! Credential store: trait + cfg-gated platform implementations.
pub mod error;

#[cfg(windows)]
mod dpapi;

#[cfg(not(windows))]
mod devfile;

pub use error::CredentialError;
```
Mirror as:
```rust
//! Printer: trait + cfg-gated platform implementations.
pub mod error;

#[cfg(windows)]
mod spooler;

#[cfg(windows)]
mod serial;

#[cfg(not(windows))]
mod stub;

pub use error::PrinterError;
```

**Trait definition** (`src/credential_store/mod.rs` lines 23–32 — mirror shape):
```rust
pub trait CredentialStore {
    fn save(&self, secret: &[u8]) -> Result<(), CredentialError>;
    fn load(&self) -> Result<Vec<u8>, CredentialError>;
}
```
Mirror as:
```rust
pub trait Printer {
    fn print_raw(&self, bytes: &[u8]) -> Result<(), PrinterError>;
}
```

**cfg-gated factory function** (`src/credential_store/mod.rs` lines 38–46 — mirror exactly):
```rust
#[cfg(windows)]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore {
    dpapi::DpapiCredentialStore::new(app_dir)
}

#[cfg(not(windows))]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore {
    devfile::DevFileCredentialStore::new(app_dir)
}
```
`printer_from_entry()` follows the same dual-cfg pattern, selecting spooler vs serial vs stub based on `PrinterId` and platform.

**Enumerate function** (new, no codebase analog — follows cfg-gated function shape from `mod.rs`):
```rust
pub fn enumerate_printers() -> Vec<PrinterEntry> {
    #[cfg(windows)]
    { windows_enumerate_printers() }
    #[cfg(not(windows))]
    { vec![] }
}
```

**PrinterError** (mirrors `CredentialError` from `src/credential_store/error.rs` lines 1–30):
```rust
// CredentialError uses thiserror 2:
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Credential file not found — agent needs activation")]
    NotFound,
    #[error("Credential file is corrupt or was encrypted by a different user (DPAPI key mismatch)")]
    Corrupt(#[source] anyhow::Error),
    #[error("I/O error accessing credential file")]
    Io(#[from] std::io::Error),
}
```
Mirror for printer:
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PrinterError {
    #[error("Printer not found or not accessible: {0}")]
    NotFound(String),
    #[error("Print job failed: {0}")]
    PrintFailed(String),
    #[error("Serial port error: {0}")]
    SerialPort(String),
    #[error("I/O error")]
    Io(#[from] std::io::Error),
}
```

---

### `src/printer/spooler.rs` (service, request-response)

**Analog:** `src/credential_store/dpapi.rs` — exact structural mirror.

**File header and cfg gate** (`src/credential_store/dpapi.rs` lines 1–11 — mirror exactly):
```rust
//! Windows DPAPI credential store implementation.
//!
//! **Windows-only.** This module is compiled only when `cfg(windows)`.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use windows_dpapi::{decrypt_data, encrypt_data, Scope};
use super::{CredentialError, CredentialStore};
```
Mirror as:
```rust
//! Windows spooler printer implementation (WritePrinter RAW path).
//!
//! **Windows-only.** This module is compiled only when `cfg(windows)`.
#![cfg(windows)]

use windows::Win32::Graphics::Printing::*;
use windows::core::PCWSTR;
use super::{PrinterError, Printer};
```

**Struct + `new()` + impl** (`src/credential_store/dpapi.rs` lines 20–51 — mirror shape):
```rust
pub struct DpapiCredentialStore {
    path: PathBuf,
}

impl DpapiCredentialStore {
    pub fn new(app_dir: &Path) -> Self {
        Self { path: app_dir.join("credential.bin") }
    }
}

impl CredentialStore for DpapiCredentialStore {
    fn save(&self, secret: &[u8]) -> Result<(), CredentialError> {
        let encrypted = encrypt_data(secret, Scope::User, None)
            .map_err(|e| CredentialError::Corrupt(anyhow::anyhow!(e)))?;
        std::fs::write(&self.path, encrypted)?;
        Ok(())
    }

    fn load(&self) -> Result<Vec<u8>, CredentialError> {
        if !self.path.exists() {
            return Err(CredentialError::NotFound);
        }
        // ...
    }
}
```
Mirror as `WindowsSpoolerPrinter { printer_name: String }` with `impl Printer for WindowsSpoolerPrinter`, delegating to `unsafe fn write_raw_to_spooler()` (the Win32 sequence from RESEARCH Pattern 3).

**Critical pitfall C1 in spooler impl** (from RESEARCH.md Pattern 3 — must annotate):
```rust
// CRITICAL (C1): pDatatype MUST be "RAW" or ESC/POS becomes silent garbage.
let datatype: Vec<u16> = "RAW\0".encode_utf16().collect();
let doc_info = DOC_INFO_1W {
    pDocName: PWSTR(doc_name.as_ptr() as *mut u16),
    pOutputFile: PWSTR::null(),
    pDatatype: PWSTR(datatype.as_ptr() as *mut u16),  // ← CRITICAL: "RAW"
};
```

---

### `src/printer/serial.rs` (service, request-response)

**Analog:** `src/credential_store/dpapi.rs` (same Windows-only impl structure).

**File header and cfg gate** (`src/credential_store/dpapi.rs` line 7 — same pattern):
```rust
#![cfg(windows)]
```

**Impl shape** mirrors `DpapiCredentialStore` but uses `serialport::open()`:
```rust
pub struct SerialPrinter {
    port_name: String,
}

impl Printer for SerialPrinter {
    fn print_raw(&self, bytes: &[u8]) -> Result<(), PrinterError> {
        let mut port = serialport::new(&self.port_name, 9600)
            .open()
            .map_err(|e| PrinterError::SerialPort(e.to_string()))?;
        use std::io::Write as _;
        port.write_all(bytes)
            .map_err(|e| PrinterError::Io(e))?;
        Ok(())
    }
}
```

---

### `src/printer/stub.rs` (service, request-response)

**Analog:** `src/credential_store/devfile.rs` — THE direct pattern source.

**File header, cfg gate, and doc comment** (`src/credential_store/devfile.rs` lines 1–11 — mirror exactly):
```rust
//! DEV/TEST ONLY — NOT A SECURE STORE. Never ships.
//!
//! Exists only to exercise the `CredentialStore` trait + error contract on Linux (D-24).
#![cfg(not(windows))]

use super::{CredentialError, CredentialStore};
```
Mirror as:
```rust
//! Linux stub printer — DEV/TEST ONLY. Never ships.
//!
//! Exists to exercise the Printer trait contract on Linux without hardware.
#![cfg(not(windows))]

use super::{PrinterError, Printer};
```

**No-op impl** (mirrors `DevFileCredentialStore` save pattern but always succeeds):
```rust
pub struct StubPrinter;

impl Printer for StubPrinter {
    fn print_raw(&self, _bytes: &[u8]) -> Result<(), PrinterError> {
        Ok(())  // No-op: Linux dev only
    }
}
```

---

### `src/noren_client.rs` (service, request-response)

**Analog:** `src/credential_store/error.rs` (typed error enum with thiserror) + `src/credential_store/mod.rs` (public API surface shape).

No HTTP client code exists in the codebase yet. The typed error enum follows the exact `CredentialError` shape.

**Error enum pattern** (`src/credential_store/error.rs` lines 6–30 — adapt variant names):
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Credential file not found — agent needs activation")]
    NotFound,
    #[error("Credential file is corrupt or was encrypted by a different user (DPAPI key mismatch)")]
    Corrupt(#[source] anyhow::Error),
    #[error("I/O error accessing credential file")]
    Io(#[from] std::io::Error),
}
```
Adapt to:
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ActivateError {
    #[error("Serial inválido")]
    InvalidSerial,
    #[error("Serial já ativo em outra máquina")]
    AlreadyActiveOther,
    #[error("Network error: {0}")]
    Transport(#[from] reqwest::Error),
}
```

**Serde structs** (no codebase analog; from RESEARCH.md Pattern 4 — use `serde` already in Cargo.toml):
```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ActivateRequest<'a> {
    serial: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    machine_id: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]  // Noren returns agentToken, tenantId, etc.
pub struct ActivateResponse {
    pub agent_token: String,
    pub tenant_id: String,
    pub pusher_key: String,
    pub pusher_cluster: String,
    pub enabled_types: Vec<String>,
}
```

---

### `src/machine_id.rs` (utility, request-response)

**Analog:** `src/credential_store/mod.rs` lines 38–46 (dual-cfg factory function pattern).

**Dual-cfg function pattern** (`src/credential_store/mod.rs` lines 38–46):
```rust
#[cfg(windows)]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore {
    dpapi::DpapiCredentialStore::new(app_dir)
}

#[cfg(not(windows))]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore {
    devfile::DevFileCredentialStore::new(app_dir)
}
```
Mirror with a single-function, same cfg-pair structure:
```rust
#[cfg(windows)]
pub fn get_machine_id() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm.open_subkey("SOFTWARE\\Microsoft\\Cryptography").ok()?;
    key.get_value("MachineGuid").ok()
}

#[cfg(not(windows))]
pub fn get_machine_id() -> Option<String> {
    None
}
```

---

### `src/main.rs` (controller, event-driven) — modify in place

**Analog:** `src/main.rs` itself (extend, not replace).

**ApplicationHandler struct** (`src/main.rs` lines 38–41 — extend):
```rust
struct App {
    window: Option<brevly_print::spike_window::SpikeWindow>,
}
```
Phase 2 change: rename field to `activation_window`, add `rt: tokio::runtime::Handle` field, replace `SpikeWindow` type with `ActivationWindow`.

**Credential check branch** (`src/main.rs` lines 144–150 — the probe becomes the real branch):
```rust
// Probe credential store: save + load through the CredentialStore trait (T-1-01).
let cred = credential_store(&app_dir);
use brevly_print::credential_store::CredentialStore as _;
cred.save(b"skeleton-dummy")
    .context("Credential save failed")?;
let loaded = cred.load().context("Credential load failed")?;
```
Phase 2 replacement:
```rust
let cred = credential_store(&app_dir);
use brevly_print::credential_store::{CredentialStore as _, CredentialError};
let needs_activation = match cred.load() {
    Ok(_token) => false,
    Err(CredentialError::NotFound) | Err(CredentialError::Corrupt(_)) => true,
    Err(e) => return Err(e).context("Credential I/O error"),
};
```

**Tokio runtime** (`src/spike_window.rs` lines 181–183 — promote to `main()` scope):
```rust
// In spike_window.rs (per-window, local, current_thread):
let rt = tokio::runtime::Builder::new_current_thread()
    .build()
    .context("Failed to build tokio runtime for wgpu init")?;
```
Phase 2: move to `main()`, use `new_multi_thread()`, keep alive for process lifetime:
```rust
// In main() before event_loop:
let rt = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .context("Failed to build tokio runtime")?;
let rt_handle = rt.handle().clone();
```

**UserEvent enum** (`src/main.rs` lines 29–33 — extend with oneshot results for Phase 2):
```rust
enum UserEvent {
    // Phase 3: TrayIconEvent(tray_icon::TrayIconEvent),
    // Phase 3: MenuEvent(tray_icon::menu::MenuEvent),
}
```
Phase 2 may use `event_loop.create_proxy()` to forward tokio task results back to the winit thread if polling via `try_recv()` proves insufficient. Prefer `try_recv()` inside `about_to_wait()` first.

**`about_to_wait()`** (`src/main.rs` lines 107–113 — extend for oneshot polling):
```rust
fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
    if let Some(spike) = self.window.as_ref() {
        spike.window().request_redraw();
    }
}
```
Phase 2: rename `spike` to `window` (matches renamed field) and add tokio result polling logic here.

---

### `tests/noren_client_test.rs` (test, request-response)

**Analog:** `tests/credential_contract_test.rs` — exact test structure.

**Test file header and import pattern** (`tests/credential_contract_test.rs` lines 1–13):
```rust
//! Integration tests for the `CredentialStore` trait contract via `DevFileCredentialStore`.
//!
//! **Linux-provable trait contract** — NOT Windows-gated.
use brevly_print::credential_store::{credential_store, CredentialError, CredentialStore};
```
Mirror as:
```rust
//! Integration tests for noren_client: ActivateRequest/ActivateResponse/ActivateError types.
//!
//! **Linux-testable** — uses a mock HTTP server, not a real Noren endpoint.
use brevly_print::noren_client::{ActivateError, ActivateResponse};
```

**Test function shape** (`tests/credential_contract_test.rs` lines 18–27):
```rust
#[test]
fn test_trait_contract_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = credential_store(dir.path());
    let secret = b"trait-contract-test-secret";
    store.save(secret).expect("save should succeed");
    let loaded = store.load().expect("load should succeed");
    assert_eq!(loaded.as_slice(), secret);
}
```
Mirror per-error-variant structure (`test_trait_contract_not_found` → `test_activate_404_returns_invalid_serial`, etc.). Use `tokio::test` attribute for async tests.

---

### `tests/printer_test.rs` (test, request-response)

**Analog:** `tests/credential_contract_test.rs` — exact test structure.

**Test for Linux stub path** (`tests/credential_contract_test.rs` lines 29–42):
```rust
#[test]
fn test_trait_contract_not_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = credential_store(dir.path());
    let result = store.load();
    assert!(
        matches!(result, Err(CredentialError::NotFound)),
        "expected NotFound, got: {result:?}"
    );
}
```
Mirror as:
```rust
#[test]
fn test_stub_printer_print_raw_returns_ok() {
    // On Linux the stub always returns Ok — no hardware needed.
    use brevly_print::printer::enumerate_printers;
    let entries = enumerate_printers();
    assert!(entries.is_empty(), "Linux stub must return empty printer list");
}
```

---

### `Cargo.toml` — modify in place

**Analog:** `Cargo.toml` lines 52–78 (`[target.'cfg(windows)'.dependencies]` block).

**Windows-only dep pattern** (`Cargo.toml` lines 66–68 — follow exactly):
```toml
# Printer enumeration (Phase 2)
printers = "2"
```
Add `winreg` immediately after `auto-launch`:
```toml
# Autostart (Phase 2)
auto-launch = "0.6"

# Machine ID from Windows registry (Phase 2)
winreg = "0.56"
```

---

### `src/lib.rs` — modify in place

**Analog:** `src/lib.rs` lines 1–12.

**Module export pattern** (`src/lib.rs` lines 7–12 — copy exactly):
```rust
pub mod app_dir;
pub mod config_store;
pub mod credential_store;
pub mod spike_window;

pub use app_dir::init_app_dir;
```
Phase 2 adds:
```rust
pub mod activation_window;
pub mod activation_state;
pub mod machine_id;
pub mod noren_client;
pub mod printer;
// Remove: pub mod spike_window;  (replaced by activation_window)
```

---

## Shared Patterns

### cfg-gating (THE project convention — apply to all Windows-only code)

**Source:** `src/credential_store/mod.rs` lines 10–14 and `src/credential_store/dpapi.rs` line 7

File-level gate (preferred for impl files that are 100% Windows-only):
```rust
// At top of file — no #[cfg] on individual items needed:
#![cfg(windows)]
```

Module-level gate in `mod.rs` (for the factory/trait declaration file):
```rust
#[cfg(windows)]
mod dpapi;

#[cfg(not(windows))]
mod devfile;
```

Function-level gate (for dual-cfg public functions in `mod.rs`):
```rust
#[cfg(windows)]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore {
    dpapi::DpapiCredentialStore::new(app_dir)
}

#[cfg(not(windows))]
pub fn credential_store(app_dir: &Path) -> impl CredentialStore {
    devfile::DevFileCredentialStore::new(app_dir)
}
```
Apply this triple pattern to: `src/printer/mod.rs`, `src/machine_id.rs`.

---

### Typed error enum with thiserror (apply to all new error types)

**Source:** `src/credential_store/error.rs` lines 6–30

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("Credential file not found — agent needs activation")]
    NotFound,
    #[error("Credential file is corrupt or was encrypted by a different user (DPAPI key mismatch)")]
    Corrupt(#[source] anyhow::Error),
    #[error("I/O error accessing credential file")]
    Io(#[from] std::io::Error),
}
```
Apply to: `src/printer/error.rs` (PrinterError), `src/noren_client.rs` (ActivateError).

---

### anyhow::Context chaining (apply to all startup/orchestration code)

**Source:** `src/main.rs` lines 11, 126–137

```rust
use anyhow::Context as _;

let app_dir = init_app_dir().context("Failed to create BrevlyPrint app directory")?;
let conn = config_store::open_and_migrate(&db_path)
    .context("Failed to open or migrate state.db")?;
```
Apply to: `src/main.rs` Phase 2 startup additions (tokio runtime creation, credential branch, activation result).

---

### Integration test structure with tempfile

**Source:** `tests/credential_contract_test.rs` and `tests/config_store_test.rs`

```rust
use brevly_print::config_store;

#[test]
fn test_write_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("state.db");
    let conn = config_store::open_and_migrate(&db_path).expect("open_and_migrate should succeed");
    config_store::set(&conn, "printer_name", "EPSON_TM_T20").expect("set should succeed");
    let got = config_store::get(&conn, "printer_name").expect("get should succeed");
    assert_eq!(got, Some("EPSON_TM_T20".to_string()));
}
```
Apply to: `tests/noren_client_test.rs`, `tests/printer_test.rs`. Use `tempfile::tempdir()` for any test needing disk isolation.

---

### config_store KV persistence (apply on activation save — D-15)

**Source:** `src/config_store.rs` lines 87–110

```rust
pub fn set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO config(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}
```
At save time, call `config_store::set` for keys: `"printer_name"`, `"printer_type"` (spooler/serial), `"tenant_id"`, `"enabled_types"`, `"noren_base_url"`.

---

### DpapiCredentialStore.save() for agentToken (apply at activation save — ACT-06)

**Source:** `src/credential_store/dpapi.rs` lines 34–38

```rust
fn save(&self, secret: &[u8]) -> Result<(), CredentialError> {
    let encrypted = encrypt_data(secret, Scope::User, None)
        .map_err(|e| CredentialError::Corrupt(anyhow::anyhow!(e)))?;
    std::fs::write(&self.path, encrypted)?;
    Ok(())
}
```
Call `cred.save(agent_token.as_bytes())` at save time. The `CredentialStore` trait is already wired — no new code needed on the trait side.

---

## No Analog Found

All new files for Phase 2 have a strong codebase analog. The table below lists the specific code patterns that have no analog in the codebase (so planner must rely on RESEARCH.md patterns instead of a codebase excerpt):

| Code Pattern | Role | Reason | Use Instead |
|--------------|------|---------|-------------|
| `reqwest` async HTTP POST | noren_client.rs | No HTTP code exists yet | RESEARCH.md Pattern 4 (reqwest with oneshot) |
| `tokio::sync::oneshot` polling in egui frame | activation_window.rs | No async egui code yet | RESEARCH.md Pattern 2 (try_recv per frame) |
| Win32 WritePrinter RAW sequence | printer/spooler.rs | No spooler code yet | RESEARCH.md Pattern 3 (full Win32 sequence) |
| `winreg` MachineGuid read | machine_id.rs | No registry code yet | RESEARCH.md Pattern 5 (winreg subkey/value) |
| `auto-launch` HKCU registration | main.rs save path | No autostart code yet | RESEARCH.md Pattern 6 (WindowsEnableMode::CurrentUser) |
| `printers` + `serialport` enumeration | printer/mod.rs | No enumeration code yet | RESEARCH.md Pattern 7 (combined Vec<PrinterEntry>) |
| egui ComboBox widget | activation_window.rs | Spike uses only TextEdit + Button | 02-UI-SPEC.md Printer ComboBox contract |
| egui Spinner widget | activation_window.rs | No spinner in spike | 02-UI-SPEC.md Buttons Row busy state |

---

## Metadata

**Analog search scope:** `src/` (all 9 source files), `tests/` (4 integration tests), `Cargo.toml`
**Files scanned:** 13 source files read in full
**Pattern extraction date:** 2026-07-15
