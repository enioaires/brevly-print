# Phase 02: Activation — Research

**Researched:** 2026-07-15
**Domain:** Windows egui activation window — serial validation (reqwest), printer enumeration (printers + serialport), raw byte printing (windows crate WritePrinter RAW), DPAPI persistence, HKCU autostart (auto-launch)
**Confidence:** HIGH (stack verified against crates.io + official docs; architecture confirmed by Phase 1 codebase)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Serial is per-machine — keep existing Noren contract (serial → one machine). Multi-register = multiple serials.
- **D-02:** 409 "serial já ativo em outra máquina" → re-bind with confirmation dialog inline (destructive red button).
- **D-04:** Validation feedback: spinner on "Ativar" while calling Noren; errors inline without closing window.
- **D-05:** Single combined dropdown with type label: "EPSON TM-T20 (USB)" / "COM3 (Serial)".
- **D-06:** Pre-select Windows default printer (`get_default_printer`) when one exists.
- **D-07:** Empty state: no printers → instruction + "Atualizar lista" button; Save disabled.
- **D-08:** Test-print = legible coupon ("Brevly Print — ativação OK" + date/time) + cut bytes (ESC @ + GS V). Mandatory step.
- **D-09:** Test-print failure does NOT hard-block save (warn-but-allow).
- **D-10:** Re-activation: all fields blank (do NOT persist serial to SQLite).
- **D-11:** Re-activation banner: "Precisamos reativar este computador — sua licença continua válida." Reassuring, muted color.
- **D-12:** Distinguish network failure from invalid serial: separate copy deck per error type + "Tentar de novo" for transport errors.
- **D-13:** Register autostart (HKCU Run via `auto-launch`) at save time. Warn-not-block on failure.
- **D-14:** Single screen layout (no wizard).
- **D-15:** After save: close window, process exits 0.

### Claude's Discretion

- Serial field style (segmented vs freeform) — D-03, pin to actual Noren serial format.
- Re-activation banner copy/behavior — D-11 (lean toward short reassuring message).
- `machineId` generation/stability — planner decides; must be stable across reboots.
- Window sizing and egui styling (branding deferred to `/gsd:ui-phase 2`).

### Deferred Ideas (OUT OF SCOPE)

- Per-tenant serial / multi-machine licensing model.
- One agent driving multiple printers (v1 = one printer).
- Branding / visual identity — routed to `/gsd:ui-phase 2` (D-16).
- Tray icon / runtime / signed installer — Phase 3. Print retry + toast — Phase 6.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| ACT-01 | Windows installer installs agent as normal program | Clarified below: installer belongs to Phase 3 (DIST-01). ACT-01 in Phase 2 = binary runs correctly post-install; no installer authoring here. |
| ACT-02 | On first run, activation window opens with serial field + printer dropdown | egui CentralPanel on winit ApplicationHandler; credential check → activation branch in main.rs |
| ACT-03 | Serial validated against Noren POST /api/agent/activate | reqwest 0.13 async POST; tokio spawn + oneshot channel to return result to egui frame |
| ACT-04 | Owner selects printer from combined list (Windows printers + COM ports) | printers 2.3.0 + serialport 4.9.0 under cfg(windows); Linux stub = empty list |
| ACT-05 | Test-print button sends raw ESC/POS bytes to printer before saving | windows crate WritePrinter with pDatatype="RAW"; serial path via serialport Write; Printer trait pattern |
| ACT-06 | agentToken stored encrypted (DPAPI), config in SQLite | DpapiCredentialStore.save() (exists, Phase 1); config_store::set() for printer name + tenantId + enabledTypes |
| ACT-07 | If credential unreadable, agent re-enters activation flow | CredentialError::NotFound/Corrupt already typed; main.rs branch to activation window |
| ACT-08 | Agent registers autostart and starts with Windows | auto-launch 0.6 AutoLaunch::new with WindowsEnableMode::CurrentUser; enable() at save time |
</phase_requirements>

---

## Summary

Phase 2 grows the Phase 1 walking-skeleton window (`spike_window.rs`) into a fully functional one-time activation screen. The codebase already has all portable foundations (winit+egui render loop, ConfigStore, CredentialStore trait + DPAPI impl, app-dir). This phase adds: (1) the activation UI state machine in place of the spike stub; (2) the first HTTP code (`reqwest` async POST to Noren); (3) printer enumeration (Windows spooler via `printers` + COM ports via `serialport`); (4) raw byte printing for the test-print (`windows` crate WritePrinter, datatype `"RAW"`); (5) autostart registration (`auto-launch` HKCU); (6) re-activation detection at startup.

All new Windows-only integrations follow the cfg-gating precedent from Phase 1: a `Printer` trait (analogous to `CredentialStore`) with a `WindowsSpoolerPrinter` impl, a `SerialPrinter` impl, and a `StubPrinter` for Linux tests. The tokio runtime already exists in main.rs (used for wgpu init) and can be promoted to a persistent `Handle` passed into the activation window state, enabling `tokio::spawn` for async HTTP without blocking the winit event thread.

The critical gotcha (C1 from STATE.md) is that `pDatatype` in `DOC_INFO_1W` MUST be set to the string `"RAW"` — omitting it causes the spooler to interpret ESC/POS bytes as GDI/EMF and produce silent garbage output. The test-print (D-08) is explicitly the validation gate for this.

**ACT-01 clarification:** The roadmap assigns the *signed installer* to Phase 3 (DIST-01). In Phase 2, ACT-01 means "the binary runs correctly as a launchable program"; no `.exe` installer is authored in this phase. The planner should flag ACT-01 as PARTIAL in Phase 2 (binary works) and FULL in Phase 3 (signed installer).

**Primary recommendation:** Grow `spike_window.rs` into `activation_window.rs`; use the existing `CredentialStore` trait to store `agentToken`; introduce a `Printer` trait in `src/printer/` following the same cfg-gate pattern; promote the tokio runtime to a persistent handle for async HTTP calls; integrate all Windows-only crates already pinned in Cargo.toml.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Activation UI (serial field, printer dropdown, buttons) | Frontend (egui/winit main thread) | — | egui is immediate-mode; all UI state lives in the ApplicationHandler |
| Serial HTTP validation (POST /api/agent/activate) | Async worker (tokio spawn) | Frontend (oneshot result) | Must not block winit event thread; result delivered via oneshot channel polled each frame |
| Printer enumeration | Windows OS / cfg(windows) stub | — | printers + serialport APIs; Linux stub returns empty Vec |
| Raw byte printing (test-print) | Windows OS / cfg(windows) stub | — | WritePrinter spooler path (USB) or serialport write (COM); Linux stub no-ops |
| agentToken persistence | Windows DPAPI / DevFile (Linux dev) | SQLite config | DpapiCredentialStore already exists; SQLite for printer + tenant config |
| Autostart registration | Windows Registry (HKCU Run) | — | auto-launch 0.6 under cfg(windows); Linux stub = no-op |
| Re-activation detection | Application startup (main.rs) | — | CredentialError::NotFound/Corrupt branch before showing activation window |

---

## Standard Stack

All packages below are already pinned in `Cargo.toml` (Phase 1 locked the dep set). No new packages need to be added for Phase 2.

### Core (Phase 2 activates these existing deps)

| Library | Pinned Version | Current (crates.io) | Purpose | Why Standard |
|---------|---------------|---------------------|---------|--------------|
| `egui` | 0.35 | 0.35 | Activation UI widgets | Already in Cargo.toml; spike proves it works on winit 0.30 |
| `egui-winit` | 0.35 | 0.35 | winit event → egui input | Same |
| `egui-wgpu` | 0.35 | 0.35 | egui → wgpu render | Same |
| `reqwest` | 0.13 | 0.13.4 | HTTP POST to Noren | Default-features=false, features=["rustls","json"]; no native-tls |
| `tokio` | 1 (full) | 1.x | Async runtime for HTTP | Already used for wgpu init block_on |
| `printers` | 2 | 2.3.0 | Windows printer enumeration | `get_printers()`, `get_default_printer()` under cfg(windows) |
| `serialport` | 4.9 | 4.9.0 | COM port enumeration + write | `available_ports()` under cfg(windows); serial write path |
| `windows` | 0.62 | 0.62.2 | WritePrinter RAW spooler | `Win32_Graphics_Printing` feature; stable Win32 API |
| `windows-dpapi` | 0.2 | 0.2.0 | DPAPI encrypt/decrypt | DpapiCredentialStore already uses this |
| `auto-launch` | 0.6 | 0.6.0 | HKCU Run autostart | `WindowsEnableMode::CurrentUser` |
| `winreg` | — (NEW) | 0.56.0 | Read MachineGuid from registry | Needed for machineId in activate request |

**Package legitimacy note:** `winreg` is a new dependency not yet in `Cargo.toml`. It requires addition to `[target.'cfg(windows)'.dependencies]`.

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `serde` / `serde_json` | 1 | Serialize activate request/response JSON | Already in Cargo.toml |
| `rusqlite` | 0.40 | Persist printer name, tenantId, enabledTypes in config table | config_store::set() calls at save time |
| `thiserror` | 2 | PrinterError type for the new Printer trait | Already used by CredentialError |
| `anyhow` | 1 | Error propagation in activation flow | Already in Cargo.toml |

### New dependency to add

```toml
[target.'cfg(windows)'.dependencies]
# Machine ID for Noren activate request (Phase 2)
winreg = "0.56"
```

**Version verification:** `cargo search winreg` confirms `winreg = "0.56.0"` is current. [VERIFIED: crates.io registry]

### Installation

No new `cargo install` needed. The single addition to `Cargo.toml` is `winreg = "0.56"` under Windows-only dependencies.

---

## Package Legitimacy Audit

> slopcheck was unavailable at research time. All packages below are assessed from official sources and crates.io registry. Packages marked `[ASSUMED]` require human verification before install.

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `printers` | crates.io | ~4 yrs | Moderate | github.com/talesluna/rust-printers | unavailable | [ASSUMED] — planner must verify before use |
| `auto-launch` | crates.io | ~4 yrs | High | github.com/Teamwork/node-auto-launch (Rust port) | unavailable | [ASSUMED] |
| `winreg` | crates.io | ~9 yrs | Very high (>1M/wk) | github.com/gentoo90/winreg-rs | unavailable | [ASSUMED] — well-established, Windows ecosystem standard |
| `reqwest` | crates.io | ~7 yrs | Very high | github.com/seanmonstar/reqwest | unavailable | [ASSUMED] — de facto HTTP client |
| `windows` | crates.io | ~4 yrs | Very high | github.com/microsoft/windows-rs | unavailable | [ASSUMED] — official Microsoft crate |
| `serialport` | crates.io | ~6 yrs | High | github.com/serialport/serialport-rs | unavailable | [ASSUMED] |
| `windows-dpapi` | crates.io | ~1-2 yrs | Low-moderate | unknown | unavailable | [ASSUMED] — Phase 1 already uses this; DPAPI proved |
| `tokio-tungstenite` | crates.io | ~5 yrs | Very high | github.com/snapview/tokio-tungstenite | unavailable | Phase 4 only — out of scope |

**Packages removed due to slopcheck [SLOP]:** none
**Packages flagged as suspicious [SUS]:** none identified (all are known ecosystem packages)

*slopcheck was unavailable — all packages above are `[ASSUMED]`. The planner should note that `printers`, `auto-launch`, and `winreg` are new activations in Phase 2 and warrant a `checkpoint:human-verify` before their first `cargo build`.*

---

## Architecture Patterns

### System Architecture Diagram

```
 main.rs startup
      │
      ├─ credential_store().load()
      │       │
      │       ├─ Ok(token) ────────────────────────────────► Phase 3 runtime (tray)
      │       │
      │       └─ Err(NotFound | Corrupt)
      │                 │
      │                 ▼
      │      ActivationWindow (winit + egui)
      │                 │
      │         ┌───────┴──────────┐
      │         │                  │
      │   [Serial input]    [Printer dropdown]
      │         │                  │
      │    "Ativar" click    get_printers()      available_ports()
      │         │            cfg(windows)         cfg(windows)
      │         │                  │                   │
      │    tokio::spawn            └──────┬────────────┘
      │    reqwest POST                   ▼
      │    /api/agent/activate    combined Vec<PrinterEntry>
      │         │
      │    oneshot::Receiver
      │    polled each frame
      │         │
      │   ┌─────┴──────────┐
      │   │ 200 OK         │ 403/404/409/transport err
      │   │ agentToken     │ inline error text
      │   │ tenantId       │
      │   │ enabledTypes   │
      │   ▼
      │  [Test-print button]
      │         │
      │   "Imprimir teste"
      │         │
      │    ┌────┴──────────────────┐
      │    │ USB spooler path      │ COM serial path
      │    │ OpenPrinterW          │ serialport::open
      │    │ StartDocPrinterW RAW  │ write bytes
      │    │ WritePrinter          │
      │    │ EndDocPrinter         │
      │    └────────────────────────┘
      │         │
      │    warn / confirm
      │         │
      │   "Salvar ativação"
      │         │
      │    ┌────┴────────────────────────────────────────┐
      │    │ 1. DpapiCredentialStore.save(agentToken)   │
      │    │ 2. config_store::set(printer_name, ...)    │
      │    │ 3. config_store::set(tenant_id, ...)       │
      │    │ 4. config_store::set(enabled_types, ...)   │
      │    │ 5. auto-launch AutoLaunch::enable()        │
      │    │ 6. event_loop.exit() → process::exit(0)   │
      │    └────────────────────────────────────────────┘
```

### Recommended Project Structure

```
src/
├── main.rs                    # startup branch: activation vs runtime
├── lib.rs                     # exports activation_window + printer modules
├── spike_window.rs            # REPLACE with activation_window.rs (or rename)
├── activation_window.rs       # ActivationWindow: egui UI + state machine
├── activation_state.rs        # ActivationUiState enum + ActivationFormState struct
├── printer/
│   ├── mod.rs                 # Printer trait + PrinterEntry + PrinterError
│   ├── spooler.rs             # cfg(windows): WindowsSpoolerPrinter (WritePrinter RAW)
│   ├── serial.rs              # cfg(windows): SerialPrinter (serialport)
│   └── stub.rs                # cfg(not(windows)): StubPrinter + empty enum list
├── machine_id.rs              # cfg(windows): read MachineGuid; cfg(not): empty string
├── noren_client.rs            # reqwest HTTP: activate(), plus ActivateResponse type
├── config_store.rs            # existing (no changes needed)
├── credential_store/          # existing (no changes needed)
└── app_dir.rs                 # existing (no changes needed)
```

### Pattern 1: Printer Trait (mirrors CredentialStore pattern)

**What:** A cfg-gated trait with two Windows impls (spooler + serial) and a Linux stub, so activation logic tests on Linux without a printer.

**When to use:** Whenever Windows-only hardware access is needed.

```rust
// Source: pattern mirrors src/credential_store/mod.rs
// src/printer/mod.rs

#[derive(Debug, Clone)]
pub struct PrinterEntry {
    /// Display string shown in UI: "EPSON TM-T20 (USB)" or "COM3 (Serial)"
    pub display_name: String,
    /// Internal identifier passed to the platform impl
    pub id: PrinterId,
}

#[derive(Debug, Clone)]
pub enum PrinterId {
    Spooler(String),   // Windows printer name
    Serial(String),    // COM port name, e.g. "COM3"
}

pub trait Printer {
    fn print_raw(&self, bytes: &[u8]) -> Result<(), PrinterError>;
}

/// Returns platform printer list. On Linux: always empty.
pub fn enumerate_printers() -> Vec<PrinterEntry> {
    #[cfg(windows)]
    { windows_enumerate_printers() }
    #[cfg(not(windows))]
    { vec![] }
}
```

### Pattern 2: Tokio Handle for Async HTTP in winit Event Loop

**What:** Keep a `tokio::runtime::Handle` in the `App` struct, spawning tasks from `ApplicationHandler` callbacks. Results returned via `tokio::sync::oneshot` channels, polled with `try_recv()` each egui frame.

**When to use:** Any async operation (HTTP, later WebSocket) initiated from the winit event thread.

```rust
// In App struct (main.rs)
struct App {
    rt: tokio::runtime::Handle,
    window: Option<ActivationWindow>,
}

// Spawning HTTP from the event thread (non-blocking):
let (tx, rx) = tokio::sync::oneshot::channel();
self.rt.spawn(async move {
    let result = noren_client::activate(serial, machine_id).await;
    let _ = tx.send(result);
});
// Store rx in activation window state

// In egui frame (polling):
if let Ok(result) = state.activate_rx.as_mut().and_then(|rx| rx.try_recv().ok()) {
    // handle result
    state.activate_rx = None;
}
```

**Critical:** The tokio runtime must be created BEFORE winit's `EventLoop` and live for the process lifetime. Use `tokio::runtime::Builder::new_multi_thread().build()` (not `new_current_thread()`), because HTTP connections use multiple threads internally. Keep the `Runtime` alive in `main()` scope and pass its `Handle` (`rt.handle().clone()`) into `App`.

**Note:** Phase 1 already uses `tokio::runtime::Builder::new_current_thread()` for wgpu init. For Phase 2 HTTP, a multi-thread runtime is better. The wgpu init tokio block_on can move to the existing runtime or remain separate — the planner should decide the cleanest split.

### Pattern 3: WritePrinter RAW (Windows spooler path)

**What:** The canonical Win32 sequence for ESC/POS bytes to a spooler-managed USB printer. Pitfall C1: `pDatatype` MUST be `"RAW"`.

```rust
// Source: Microsoft Learn / win32-raw-data-to-printer (confirmed)
// Adapted for windows crate 0.62 (unsafe required)
#[cfg(windows)]
unsafe fn write_raw_to_spooler(printer_name: &str, data: &[u8]) -> Result<(), PrinterError> {
    use windows::Win32::Graphics::Printing::*;
    use windows::core::PCWSTR;

    let name_w: Vec<u16> = printer_name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut handle = PRINTER_HANDLE::default();
    OpenPrinterW(PCWSTR(name_w.as_ptr()), &mut handle, None)?;

    let doc_name: Vec<u16> = "BrevlyPrint\0".encode_utf16().collect();
    let datatype: Vec<u16> = "RAW\0".encode_utf16().collect();  // ← CRITICAL
    let doc_info = DOC_INFO_1W {
        pDocName: PWSTR(doc_name.as_ptr() as *mut u16),
        pOutputFile: PWSTR::null(),
        pDatatype: PWSTR(datatype.as_ptr() as *mut u16),
    };
    let job_id = StartDocPrinterW(handle, 1, &doc_info as *const _ as *const u8)?;
    if job_id == 0 { /* error */ }

    StartPagePrinterW(handle)?;
    let mut written = 0u32;
    WritePrinter(handle, data.as_ptr() as *const _, data.len() as u32, &mut written)?;
    EndPagePrinterW(handle)?;
    EndDocPrinterW(handle)?;
    ClosePrinterW(handle)?;
    Ok(())
}
```

**Note on windows crate API:** `StartDocPrinterW` signature in windows 0.62 takes `*const DOC_INFO_1W` (the `level` parameter is `1`). `ClosePrinter` may be `ClosePrinterW` or just `ClosePrinter` — check windows-docs-rs for the exact name under `Win32::Graphics::Printing`.

### Pattern 4: Noren HTTP Client

```rust
// src/noren_client.rs
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ActivateRequest<'a> {
    serial: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    machine_id: Option<&'a str>,
}

#[derive(Deserialize)]
pub struct ActivateResponse {
    pub agent_token: String,
    pub tenant_id: String,
    pub pusher_key: String,
    pub pusher_cluster: String,
    pub enabled_types: Vec<String>,
}

#[derive(Debug)]
pub enum ActivateError {
    InvalidSerial,          // 403 or 404
    AlreadyActiveOther,     // 409 — show re-bind dialog
    Transport(reqwest::Error),
}

pub async fn activate(
    base_url: &str,
    serial: &str,
    machine_id: Option<&str>,
) -> Result<ActivateResponse, ActivateError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/agent/activate"))
        .json(&ActivateRequest { serial, machine_id })
        .send()
        .await
        .map_err(ActivateError::Transport)?;

    match resp.status().as_u16() {
        200 => resp.json::<ActivateResponse>().await.map_err(ActivateError::Transport),
        403 | 404 => Err(ActivateError::InvalidSerial),
        409 => Err(ActivateError::AlreadyActiveOther),
        _ => Err(ActivateError::Transport(resp.error_for_status().unwrap_err())),
    }
}
```

**Noren contract note:** The response fields from `noren-contract-brief.md §3` are camelCase (`agentToken`, `tenantId`, etc.). Use `#[serde(rename_all = "camelCase")]` on `ActivateResponse` or rename fields individually.

### Pattern 5: machineId — Windows Registry MachineGuid

```rust
// src/machine_id.rs
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
    None  // Linux dev: omit machineId from activate request
}
```

**MachineGuid stability:** Generated at Windows installation time. Survives hardware changes; changes only on OS reinstall. Exactly the right granularity for the re-bind use case (D-02): if the owner reinstalls Windows and the DPAPI credential is lost (ACT-07), the MachineGuid also changes — the 409 re-bind flow correctly handles this.

**Registry path:** `HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Cryptography\MachineGuid` (string value).

### Pattern 6: autostart registration

```rust
// At save time, under cfg(windows)
#[cfg(windows)]
fn register_autostart(exe_path: &str, app_name: &str) -> Result<(), auto_launch::Error> {
    use auto_launch::{AutoLaunch, WindowsEnableMode};
    let al = AutoLaunch::new(app_name, exe_path, WindowsEnableMode::CurrentUser, &[]);
    al.enable()
}
```

**`WindowsEnableMode::CurrentUser`** is mandatory (not `Dynamic`, not `System`) — the agent is a per-user tray app; HKLM requires admin elevation. [VERIFIED: docs.rs/auto-launch/0.6.0]

**Exe path:** Use `std::env::current_exe()` at save time to get the installed binary path.

### Pattern 7: Printer enumeration with combined list

```rust
// src/printer/mod.rs (Windows path)
#[cfg(windows)]
fn windows_enumerate_printers() -> Vec<PrinterEntry> {
    use printers::{get_printers, get_default_printer};
    use serialport::available_ports;

    let default_name = get_default_printer().map(|p| p.name.clone());
    let mut entries = Vec::new();

    // Spooler printers
    for p in get_printers() {
        entries.push(PrinterEntry {
            display_name: format!("{} (USB)", p.name),
            id: PrinterId::Spooler(p.name.clone()),
            is_default: default_name.as_deref() == Some(&p.name),
        });
    }

    // COM ports
    if let Ok(ports) = available_ports() {
        for port in ports {
            entries.push(PrinterEntry {
                display_name: format!("{} (Serial)", port.port_name),
                id: PrinterId::Serial(port.port_name.clone()),
                is_default: false,
            });
        }
    }

    entries
}
```

**Note on `printers` API:** `get_printers()` returns `Vec<Printer>` and `get_default_printer()` returns `Option<Printer>`. The `Printer` struct has a `name: String` field (the human-visible printer name registered in Windows). Accessing `p.name` is the correct pattern. [CITED: docs.rs/printers/latest]

**Note on `serialport` API:** `available_ports()` returns `Result<Vec<SerialPortInfo>, serialport::Error>`. `SerialPortInfo.port_name` is the COM port string (e.g., `"COM3"`). [ASSUMED — confirmed from crate description and common usage pattern]

### Anti-Patterns to Avoid

- **Blocking the winit thread for HTTP:** Never call `block_on(activate())` from `window_event()` — the window will freeze during the network round-trip. Always `tokio::spawn` and poll with `try_recv()`.
- **Omitting pDatatype="RAW":** Pitfall C1. The spooler defaults to a GDI/EMF interpretation. ESC/POS bytes become silent garbage. Always set `pDatatype = "RAW"`.
- **Using `Dynamic` or `System` mode for autostart:** Requires admin elevation. The agent is a user-level tray app. Use `WindowsEnableMode::CurrentUser`.
- **Storing the serial in SQLite:** D-10 explicitly prohibits this. If the owner re-activates (D-10), the form must be blank.
- **Blocking save on test-print failure:** D-09. Test-print is a required step, but its success is not a save gate.
- **Putting activation config in a separate SQLite table:** Use the existing `config` KV table (`config_store::set`). No migration needed.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| DPAPI encrypt/decrypt | Custom encryption | `windows-dpapi` (already in Phase 1) | Win32 CryptProtectData handles key derivation, entropy, and blob format |
| Windows printer enumeration | EnumPrintersW manually | `printers` crate | Handles all winspool details; `get_default_printer()` is one call |
| COM port enumeration | Registry walk | `serialport::available_ports()` | Cross-platform correct; handles device detection |
| Raw bytes to spooler | Custom spooler protocol | `windows` crate `WritePrinter` | Win32 stable API; pitfall C1 already documented |
| HKCU autostart | Direct winreg calls | `auto-launch` (WindowsEnableMode::CurrentUser) | Handles Task Manager's `StartupApproved` key; correct HKCU scoping |
| Async HTTP | Hand-rolled TCP | `reqwest` 0.13 with `rustls` | TLS, JSON, error handling, connection pooling included |
| Machine ID derivation | Hash of hardware serials | `winreg` read of `MachineGuid` | Stable Windows-provided identifier; requires only one registry read |
| egui button state management | Complex UI framework | egui `ui.add_enabled()` / `ui.add_space()` | Immediate-mode; no state synchronization problem |

**Key insight:** The Windows printing stack has many silent failure modes (wrong datatype, handle not closed, printer name not exact-match). Rely on the well-tested crate surface and document the required sequence explicitly.

---

## Common Pitfalls

### Pitfall 1 (C1): WritePrinter without "RAW" datatype
**What goes wrong:** ESC/POS bytes are interpreted by the GDI print processor as EMF/GDI commands and produce silent garbage output or no output at all.
**Why it happens:** Windows print spooler defaults to the printer's registered datatype (often "NT EMF 1.008"), not raw passthrough.
**How to avoid:** Set `DOC_INFO_1W.pDatatype = "RAW"` explicitly. The test-print (D-08) is the validation gate — if the coupon text is legible, the RAW datatype is working.
**Warning signs:** Printer makes mechanical sounds but nothing prints, or output looks like random characters.

### Pitfall 2: Blocking the winit event thread
**What goes wrong:** Calling `reqwest::blocking::Client::post()` or `rt.block_on(activate())` from `window_event()` freezes the window — the OS event pump stops; Windows may show "Not Responding".
**Why it happens:** winit's event loop IS the main thread; any blocking call starves it.
**How to avoid:** Always `rt.spawn(async { ... })` + oneshot channel. Poll `rx.try_recv()` each egui frame (in `about_to_wait` or inside the `CentralPanel` closure).
**Warning signs:** Window unresponsive during HTTP call; spinner not animating.

### Pitfall 3: tokio multi-thread vs current-thread runtime
**What goes wrong:** `new_current_thread()` runtime for HTTP may deadlock if reqwest's internal connection pool spawns worker tasks that need the same thread.
**Why it happens:** `new_current_thread()` only drives the async executor on the calling thread. reqwest with rustls internally uses a threadpool via `tokio::spawn`.
**How to avoid:** Use `tokio::runtime::Builder::new_multi_thread().build()` for the persistent runtime. Keep the Phase 1 `new_current_thread()` block only for the wgpu init sequence (which is already done before the event loop).
**Warning signs:** HTTP call hangs indefinitely even with a valid server.

### Pitfall 4: auto-launch using "Dynamic" mode and triggering UAC prompt
**What goes wrong:** `WindowsEnableMode::Dynamic` tries HKLM first (requires admin). On a non-admin user, Windows shows a UAC elevation dialog unexpectedly.
**Why it happens:** `Dynamic` is the default; it falls back to HKCU only if HKLM fails.
**How to avoid:** Explicitly use `WindowsEnableMode::CurrentUser`. Register failure as warn-not-block (D-13).
**Warning signs:** UAC dialog appears when saving activation; or autostart silently fails.

### Pitfall 5: printers crate returns names that don't match OpenPrinterW
**What goes wrong:** `get_printers()` returns a display name that differs from the canonical printer name expected by `OpenPrinterW` (e.g., driver appended info).
**Why it happens:** Windows printer names can have suffixes added by the driver.
**How to avoid:** Use `p.name` from the `printers::Printer` struct directly as the argument to `OpenPrinterW`. Do not transform the name.
**Warning signs:** `OpenPrinterW` returns error 1801 (`ERROR_INVALID_PRINTER_NAME`).

### Pitfall 6: reqwest Client creation per-request
**What goes wrong:** Creating a new `reqwest::Client` for each HTTP call bypasses connection pooling; on Windows, TLS handshake adds ~200–500ms per call.
**Why it happens:** `reqwest::Client::new()` is cheap but creates a new connection pool each time.
**How to avoid:** Create the `Client` once (or use `Client::builder().build()?` once at startup) and share it via `Arc<Client>` into spawned tasks. For Phase 2 (one activate call + one optional re-bind), the performance impact is minor but the pattern should be established correctly.
**Warning signs:** Activate call takes >2 seconds on a fast connection.

### Pitfall 7: ActivateResponse field name mismatch (camelCase vs snake_case)
**What goes wrong:** Noren returns `{"agentToken": ..., "tenantId": ...}` but Rust struct has `agent_token`, `tenant_id`. `serde_json` produces deserialization error.
**Why it happens:** Rust convention is snake_case; JSON convention (and Noren) is camelCase.
**How to avoid:** Add `#[serde(rename_all = "camelCase")]` to `ActivateResponse` or use `#[serde(rename = "agentToken")]` per field.
**Warning signs:** `serde_json` returns `missing field 'agent_token'`.

### Pitfall 8: Closing the window vs exiting the process
**What goes wrong:** Calling `event_loop.exit()` returns from `run_app()` but doesn't flush SQLite WAL or guarantee DPAPI write is complete.
**Why it happens:** `event_loop.exit()` sets a flag; the actual OS process exit may happen before pending I/O completes.
**How to avoid:** Perform all persistence (DPAPI save, SQLite set, autostart register) in a synchronous save function BEFORE calling `event_loop.exit()`. D-15 says "window closes, process exits 0" — ensure this is `std::process::exit(0)` after the save function returns Ok.

---

## Code Examples

### Complete ESC/POS test-print coupon bytes

```rust
// Source: ESC/POS command reference (standard for thermal printers)
// Coupon: "Brevly Print — ativação OK\n{DD/MM/YYYY HH:MM}" + cut
fn build_test_coupon() -> Vec<u8> {
    use std::time::SystemTime;
    let now = chrono::Local::now();
    let date_str = now.format("%d/%m/%Y %H:%M").to_string();
    let text = format!("Brevly Print - ativacao OK\n{date_str}\n\n\n");

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x1b\x40");          // ESC @ — initialize printer
    bytes.extend_from_slice(text.as_bytes());
    // Note: thermal printers use ISO-8859-1 not UTF-8; "ã", "ç" etc need
    // encoding if the printer is not in UTF-8 mode. For the test coupon,
    // use ASCII-safe text (no accented chars) or encode explicitly.
    bytes.extend_from_slice(b"\x1d\x56\x00");      // GS V 0 — full cut
    bytes
}
```

**Encoding note:** The test coupon text should be ASCII-safe to avoid encoding ambiguity. Save PT-BR accented characters for Phase 5 (where Noren sends pre-encoded ESC/POS bytes). For "ativação" → use "ativacao" in the test coupon, or encode properly with the printer's expected charset. The UI string "Brevly Print — ativação OK" (from 02-UI-SPEC.md) is for the screen; the printed text can be simpler ASCII.

### egui frame polling pattern

```rust
// Inside the egui CentralPanel closure (each frame):
// Source: adapted from tokio::sync::oneshot documentation
if let Some(rx) = &mut state.activate_rx {
    match rx.try_recv() {
        Ok(Ok(response)) => {
            state.agent_token = Some(response.agent_token);
            state.tenant_id = Some(response.tenant_id);
            state.is_busy = false;
            state.activate_rx = None;
            state.flow_state = FlowState::ValidatedAwaitingTestPrint;
        }
        Ok(Err(ActivateError::InvalidSerial)) => {
            state.serial_error = Some("Serial inválido. Verifique o código e tente de novo.".into());
            state.is_busy = false;
            state.activate_rx = None;
        }
        Ok(Err(ActivateError::AlreadyActiveOther)) => {
            state.show_rebind_confirm = true;
            state.is_busy = false;
            state.activate_rx = None;
        }
        Ok(Err(ActivateError::Transport(_))) => {
            state.serial_error = Some("Sem conexão com o servidor — verifique a internet.".into());
            state.is_busy = false;
            state.activate_rx = None;
        }
        Err(oneshot::error::TryRecvError::Empty) => { /* still waiting */ }
        Err(oneshot::error::TryRecvError::Closed) => {
            state.is_busy = false;
            state.activate_rx = None;
        }
    }
}
// Request a repaint so the spinner animates while waiting
if state.is_busy { ctx.request_repaint(); }
```

---

## ACT-01 Clarification: Installer Scope

**ACT-01** ("Windows installer installs agent as normal program, appears in Add/Remove Programs") maps to Phase 2 in REQUIREMENTS.md, but ROADMAP.md assigns DIST-01 ("signed installer") to Phase 3.

**Resolution:** Phase 2 does NOT author a `.exe` installer. Phase 2 delivers a working binary that, when run, completes activation correctly. The installer wrapper (velopack `vpk` + Authenticode signing + SmartScreen considerations) is Phase 3 scope. The planner should mark ACT-01 as **partially satisfied** in Phase 2 (binary works) and **fully satisfied** in Phase 3 (signed installer + Add/Remove Programs entry).

This is a pre-existing roadmap split, not a new deferral. The Phase 2 Success Criteria item 1 ("The Windows installer can be downloaded and installs the agent") in the ROADMAP aligns with Phase 3 DIST-01 — the planner should note this discrepancy and track it in the plan header.

---

## Runtime State Inventory

> Rename/refactor/migration check: Phase 2 is not a rename phase. However, the `spike_window.rs` file will be replaced by `activation_window.rs`.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | `state.db` config table: `skeleton_probe` key from Phase 1 | Code-only — no cleanup needed; `skeleton_probe` is a test artifact, not production state |
| Live service config | None — Noren `POST /api/agent/activate` does not exist yet (blocker) | Human coordination with Noren team |
| OS-registered state | None — no autostart registered yet (first time) | None |
| Secrets/env vars | `credential.bin` (DPAPI) from Phase 1 probe: contains `b"skeleton-dummy"` | Will be overwritten by real `agentToken` on first save; not a blocker |
| Build artifacts | `spike_window.rs` will be renamed/replaced | Source-only change; lib.rs module export must be updated |

**Nothing found in most categories** — Phase 2 is greenfield for the activation flow. The `spike_window.rs` → `activation_window.rs` rename is the only structural file change.

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | Cargo build | ✓ | 1.97.0 (Linux) | — |
| cargo | Build + test | ✓ | 1.97.0 | — |
| Windows OS | DPAPI, WritePrinter, printers, auto-launch | ✗ (CI Linux) | — | cfg-gated stubs (Phase 1 precedent) |
| Noren `POST /api/agent/activate` | ACT-03 end-to-end test | ✗ (not live yet) | — | Mock server / hardcoded test token for unit tests |
| Thermal printer hardware | ACT-05 test-print | ✗ (Linux dev) | — | cfg-gated stub; Windows-only manual verification |

**Missing dependencies with no fallback on Linux:**
- Windows OS (for DPAPI, WritePrinter, printers, auto-launch) — by design; all gated with `#[cfg(windows)]`

**Missing dependencies with fallback:**
- Noren `/api/agent/activate` — unit tests use mock; integration test deferred until Noren endpoint is live

---

## Validation Architecture

> nyquist_validation is enabled (config.json `workflow.nyquist_validation: true`).

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `cargo test` |
| Config file | none (uses Cargo.toml test discovery) |
| Quick run command | `cargo test --lib` (portable core, Linux) |
| Full suite command | `cargo test` (all integration tests) |
| Windows-only verification | Manual checkpoint on Windows hardware |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | Linux-testable? |
|--------|----------|-----------|-------------------|-----------------|
| ACT-02 | Activation window opens on CredentialError | unit (logic) | `cargo test --lib activation` | ✓ (stub window, no GPU) |
| ACT-03 | Serial validates → ActivateResponse deserialized | unit (noren_client) | `cargo test --lib noren_client` | ✓ (mock HTTP) |
| ACT-03 | 403/404 → InvalidSerial error | unit | `cargo test --lib noren_client` | ✓ |
| ACT-03 | 409 → AlreadyActiveOther error | unit | `cargo test --lib noren_client` | ✓ |
| ACT-03 | Transport error → Transport variant | unit | `cargo test --lib noren_client` | ✓ |
| ACT-04 | enumerate_printers() returns empty on Linux stub | unit | `cargo test --lib printer` | ✓ |
| ACT-04 | PrinterEntry display_name format "(USB)"/"(Serial)" | unit | `cargo test --lib printer` | ✓ (with mock entries) |
| ACT-05 | StubPrinter.print_raw() returns Ok | unit | `cargo test --lib printer` | ✓ |
| ACT-05 | WritePrinter RAW with "RAW" datatype | manual | Windows hardware checkpoint | ✗ (Windows only) |
| ACT-06 | agentToken persisted via CredentialStore | unit | `cargo test --lib` (existing trait tests) | ✓ |
| ACT-06 | config_store::set persists printer_name + tenant_id | integration | `cargo test tests/config_store_test.rs` | ✓ |
| ACT-07 | CredentialError::NotFound routes to activation | unit | `cargo test --lib` (credential_contract_test) | ✓ |
| ACT-07 | CredentialError::Corrupt routes to activation | unit | `cargo test --lib` (credential_contract_test) | ✓ |
| ACT-08 | StubAutoLaunch::enable() no-op on Linux | unit | `cargo test --lib autostart` | ✓ |
| ACT-08 | AutoLaunch registers HKCU Run on Windows | manual | Windows checkpoint | ✗ (Windows only) |

### Sampling Rate

- **Per task commit:** `cargo test --lib` (portable core only, ~5 sec)
- **Per wave merge:** `cargo test` (all tests including integration)
- **Phase gate:** Full suite green on Linux + manual Windows checklist before `/gsd:verify-work`

### Windows Manual Checklist (Phase Gate)

These behaviors are not testable on Linux and require a Windows machine:

- [ ] `cargo build --target x86_64-pc-windows-msvc` succeeds (no Windows-only compile errors)
- [ ] Activation window opens on first launch (no credential)
- [ ] Serial validation calls Noren (requires Noren endpoint live)
- [ ] Printer dropdown shows installed USB printers AND COM ports with "(USB)"/"(Serial)" labels
- [ ] Test-print sends visible coupon to thermal printer + paper cut
- [ ] Save persists credential.bin (DPAPI) + config keys in state.db
- [ ] Autostart entry appears in Task Manager Startup tab
- [ ] Second launch → no activation window (credential present)
- [ ] Delete credential.bin → re-activation banner appears, fields blank

### Wave 0 Gaps

- [ ] `tests/noren_client_test.rs` — covers ACT-03 (mock HTTP responses using `reqwest` mock or a local stub server)
- [ ] `tests/printer_test.rs` — covers ACT-04/ACT-05 (Linux stub path)
- [ ] `src/printer/mod.rs` — Printer trait + PrinterEntry + StubPrinter
- [ ] `src/noren_client.rs` — ActivateRequest/ActivateResponse/ActivateError types
- [ ] `src/machine_id.rs` — MachineGuid reader

---

## Security Domain

> `security_enforcement` not explicitly set to false in config.json — enforced.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | Yes (agentToken mint) | Noren validates serial server-side; token stored via DPAPI |
| V3 Session Management | Partial (token storage) | DPAPI Scope::User; file permissions via Windows ACL |
| V4 Access Control | No | Single-user agent; no multi-user access control |
| V5 Input Validation | Yes (serial input) | Serial is sent as-is to Noren; Noren validates. Client: trim whitespace, enforce reasonable max length (e.g., 64 chars) |
| V6 Cryptography | Yes (DPAPI) | Never hand-roll; always use `windows-dpapi` crate |

### Known Threat Patterns for This Stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Serial phishing (fake Noren endpoint) | Spoofing | reqwest with `rustls` + `webpki-roots` validates TLS cert against system roots; use `https://` Noren URL from config |
| Token exfiltration via plaintext storage | Info Disclosure | DPAPI Scope::User — only the same Windows user can decrypt; `credential.bin` never logged |
| Token left in memory after process exit | Info Disclosure | agentToken held only in activation window state; process exits 0 immediately after save |
| Autostart persistence abuse | Elevation of Privilege | HKCU Run (not HKLM) — no admin privilege; user can remove via Task Manager |
| Replay of 409 re-bind | Spoofing | Noren invalidates old token on re-bind; re-bind requires current session's serial knowledge |
| Config KV keys stored in plaintext SQLite | Info Disclosure | SQLite config stores printer name + tenantId (non-secret); agentToken is NOT stored in SQLite, only in DPAPI credential.bin |

**Security note:** The `machineId` (MachineGuid) in the activate request is not a secret — it disambiguates machines for the 409 re-bind flow. Do not treat it as confidential.

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `tao` event loop | `winit 0.30` ApplicationHandler | Phase 1 decision | tao uses closure-based API incompatible with egui-winit 0.35; winit 0.30 is the correct pair |
| `eframe` for egui window | Raw `egui-winit` + `egui-wgpu` | Phase 1 decision | eframe conflicts with tray-icon event loop (C2); raw approach confirmed working |
| `native-windows-gui` activation form | `egui` on winit | Phase 1 research | NWG in permanent maintenance mode; egui is actively maintained |
| `pusher-rs` / `pusher` client | Hand-rolled over `tokio-tungstenite` | Phase 1 research | Both pusher crates are unsupported or in heavy development |
| EV certificate for SmartScreen bypass | OV certificate (same reputation) | March 2024 | EV no longer instant-bypass; OV builds reputation identically — Phase 3 concern |

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `printers::Printer.name` is the field to pass to `OpenPrinterW` (exact winspool name match) | Pattern 7 + Pitfall 5 | Wrong field name → `OpenPrinterW` fails with ERROR_INVALID_PRINTER_NAME at test-print time |
| A2 | `serialport::SerialPortInfo.port_name` is the field holding "COM3"-style name | Pattern 7 | Wrong field name → COM port path incorrect |
| A3 | Noren response JSON uses camelCase (`agentToken`, `tenantId`, `pusherKey`, `pusherCluster`, `enabledTypes`) | Pattern 4 | Wrong casing → serde deserialization fails silently or with error |
| A4 | `reqwest` "rustls" feature in 0.13 uses `webpki-roots` for cert validation without `native-tls` | Standard Stack | If cert roots need explicit `webpki-roots` feature → TLS handshake fails against Noren's cert |
| A5 | `auto-launch 0.6` `AutoLaunch::new()` takes `(name, exe_path, WindowsEnableMode, args)` parameter order | Pattern 6 | Wrong signature → compile error or incorrect autostart target |
| A6 | `tokio::sync::oneshot::Receiver::try_recv()` is the correct non-blocking poll method | Pattern 2 | Wrong method → panic or deadlock in egui frame |
| A7 | Noren Pusher serial format is variable-length (freeform), not fixed-segment (D-03 discretion) | Serial field | Wrong assumption → wrong egui widget choice; planner must confirm with Noren before implementing |

---

## Open Questions

1. **Noren serial format (D-03)**
   - What we know: D-03 defers to the planner; the UI-SPEC supports both freeform and segmented.
   - What's unclear: Is the serial a fixed-length string like `XXXX-XXXX-XXXX` or variable?
   - Recommendation: Planner should check the Noren `agent_serials` schema or ask the project owner before implementing the serial TextEdit widget. Default to freeform (single TextEdit, accept paste) until confirmed.

2. **Noren base URL for activate endpoint**
   - What we know: The contract says `POST /api/agent/activate`. No base URL is specified in any planning doc.
   - What's unclear: Is the URL hard-coded in the binary or read from a config key? (Phase 3 auto-update implies a consistent URL.)
   - Recommendation: Hard-code the Noren production URL as a compile-time constant for Phase 2 (e.g., `env!("NOREN_BASE_URL")` or literal). Add a config key `noren_base_url` in SQLite for future flexibility.

3. **ACT-01 installer scope**
   - What we know: REQUIREMENTS.md maps ACT-01 to Phase 2, but ROADMAP assigns DIST-01 (signed installer) to Phase 3.
   - Recommendation: Phase 2 plan marks ACT-01 as PARTIAL (binary functional); Phase 3 plan marks it FULL (signed installer). Document this explicitly in the plan header.

4. **reqwest Client lifecycle**
   - What we know: The activate call is a one-shot in Phase 2; the 409 re-bind adds a second call.
   - What's unclear: Whether to build Client once at app startup and share via Arc, or create per-call.
   - Recommendation: Create once at activation window construction time and store in window state. Prepares for Phase 4 (Pusher auth uses same reqwest Client).

5. **Noren endpoint liveness**
   - What we know: `POST /api/agent/activate` + `agent_serials` table are listed as blockers in STATE.md.
   - Recommendation: The plan should gate the activation integration test (Windows manual checklist item) on Noren endpoint availability. All unit tests (mock HTTP) can proceed immediately.

---

## Sources

### Primary (HIGH confidence)
- Microsoft Learn: StartDocPrinter / DOC_INFO_1 — confirmed `pDatatype = "RAW"` requirement [CITED: learn.microsoft.com/en-us/windows/win32/printdocs/startdocprinter]
- Microsoft Learn: DOC_INFO_1 structure — confirmed pDatatype field [CITED: learn.microsoft.com/en-us/windows/win32/printdocs/doc-info-1]
- Microsoft Learn: Win32 raw data to printer C example — canonical sequence confirmed [CITED: learn.microsoft.com/en-us/previous-versions/troubleshoot/windows/win32/win32-raw-data-to-printer]
- docs.rs/auto-launch/0.6.0 — WindowsEnableMode variants (Dynamic, CurrentUser, System) [CITED: docs.rs/auto-launch/0.6.0]
- docs.rs/reqwest/0.13 — "rustls" feature name confirmed (not "rustls-tls") [CITED: docs.rs/reqwest/0.13.0]
- Phase 1 codebase (`src/credential_store/`, `src/config_store.rs`, `src/spike_window.rs`, `src/main.rs`) — existing patterns [VERIFIED: codebase]
- Cargo.toml — all Phase 2 deps already pinned; printers=2, serialport=4.9, auto-launch=0.6, winreg added [VERIFIED: codebase + crates.io]
- windows-docs-rs StartDocPrinterW signature — `*const DOC_INFO_1W` at level=1 [CITED: microsoft.github.io/windows-docs-rs]

### Secondary (MEDIUM confidence)
- docs.rs/printers/latest — get_printers() returns Vec<Printer>, get_default_printer() returns Option<Printer>, Printer has .name field [CITED: docs.rs/printers]
- dev.to MachineGuid in Rust — HKLM\\SOFTWARE\\Microsoft\\Cryptography\\MachineGuid via winreg [CITED: dev.to/veer66]
- crates.io cargo search — winreg=0.56.0 current, auto-launch=0.6.0 current, printers=2.3.0 current [VERIFIED: crates.io registry]
- egui discussions #521 — tokio handle + oneshot + try_recv() pattern for async in egui [CITED: github.com/emilk/egui/discussions/521]

### Tertiary (LOW confidence)
- serialport `SerialPortInfo.port_name` field name [ASSUMED — from common usage; verify against docs.rs/serialport]
- Noren JSON response uses camelCase [ASSUMED — standard JSON/JS convention; verify against actual Noren endpoint when live]
- Noren serial format (variable vs fixed length) [ASSUMED freeform until D-03 is resolved]

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all crates already in Cargo.toml, versions verified on crates.io, APIs confirmed via official docs
- Architecture: HIGH — follows Phase 1 established patterns (cfg-gating, trait + impl, rusqlite KV)
- Pitfalls: HIGH — C1 (RAW datatype) verified against official Microsoft docs; others from direct API inspection
- Printer API field names: MEDIUM — `printers` crate docs partially unavailable; field names assumed from common usage

**Research date:** 2026-07-15
**Valid until:** 2026-09-15 (stable crates; Noren contract is the fastest-moving external dependency)
