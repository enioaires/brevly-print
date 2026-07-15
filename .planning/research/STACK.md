# Technology Stack

**Project:** Brevly Print — Native Windows Thermal Print Agent
**Researched:** 2026-07-15
**Overall confidence:** MEDIUM-HIGH (all crates verified against crates.io and docs.rs; Pusher client is the weakest area)

---

## Recommended Stack

### System Tray + Event Loop

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `tray-icon` | 0.21.x (latest on crates.io as of research date) | System tray icon with color-state, tooltip, right-click menu | Maintained by tauri-apps; works without a webview; first-class Windows support via win32 event loop |
| `tao` | 0.35.x | Win32 event loop that pumps tray-icon events | Also maintained by tauri-apps; the recommended pairing with `tray-icon` per the crate's own examples |

**Rationale:** `tray-icon` is the canonical Rust tray crate in 2025. It requires a win32 message loop running on the thread where the icon is created — `tao` provides exactly that without pulling in a webview. `winit` is an alternative event-loop crate but is actively moving toward a different design philosophy (no built-in menu support) and `tray-icon` officially recommends `tao` in its examples. Do NOT use `tao` as the source of the tray icon itself (that was the old Tauri internal API); use `tray-icon` directly and pump the loop with `tao::EventLoop`.

**How the thread model works:** `tao::EventLoop` blocks the main thread. Spawn all async work on a separate `tokio` runtime thread. Cross-thread communication via `EventLoopProxy` or a channel.

**Avoid:** `trayicon` (Ciantic/trayicon-rs) — unmaintained, last commit 2022.

---

### Activation Window (Single-Use GUI)

**Recommendation: `egui` via `eframe`**

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `egui` | 0.31.x | Immediate-mode GUI widgets (text input, combo box, button) | Pure Rust, no webview, ships its own renderer (wgpu/OpenGL), actively maintained |
| `eframe` | 0.31.x | Native window host for egui on Windows | Official egui native backend; creates a real Win32 window |

**Option comparison:**

| Option | Verdict | Reason |
|--------|---------|--------|
| `egui` + `eframe` | **RECOMMENDED** | Pure Rust, tiny binary delta, actively maintained (Emil Ernerfeldt), one-window use is trivially easy |
| `native-windows-gui` (NWG) | Not recommended | Declared "3rd and final version", in maintenance mode, future development moved elsewhere — fine today but a liability in 2–3 years. Also requires hand-writing Win32 dialog layout. |
| Raw `windows` crate Win32 dialogs | Avoid | Massive boilerplate for a single activation form; no benefit over egui for this use case |
| Tauri | Hard no | Pulls WebView2 runtime — contradicts the constraint "no webview" |

**Note on egui appearance:** egui does not use native Win32 controls, so it will not look like a native Windows dialog. For an activation form shown once by a tech-savvy installer, this is acceptable. The alternative (NWG) uses native controls but is effectively abandoned.

**Window lifecycle:** Call `eframe::run_native()` on demand (first-run only), then exit. The tray runs on `tao`'s event loop on the main thread; spawn the egui window from a dedicated thread using `eframe::run_native()` in blocking mode, or restructure so tao is in a thread and egui/eframe owns the main thread. This is the single trickiest integration point — see PITFALLS.md.

---

### Printing: Raw Bytes to Windows Printer (USB via Spooler)

**Recommendation: direct `windows` crate bindings**

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `windows` | 0.62.x | Win32 print spooler API: `OpenPrinterW`, `StartDocPrinterW`, `WritePrinter`, `EndDocPrinter` | Official Microsoft-maintained bindings; all winspool functions are available under `Win32::Graphics::Printing` feature flag |

**Feature flag to enable in Cargo.toml:**
```toml
[dependencies.windows]
version = "0.62"
features = [
  "Win32_Graphics_Printing",   # OpenPrinterW, StartDocPrinterW, WritePrinter, EndDocPrinterW
  "Win32_Foundation",           # BOOL, HANDLE, etc.
]
```

**Workflow for USB printer (appears as installed Windows printer):**
1. `OpenPrinterW(printer_name)` → PRINTER_HANDLE
2. Fill `DOC_INFO_1W { pDocName, pDatatype: "RAW" }`
3. `StartDocPrinterW(handle, 1, &doc_info)`
4. `StartPagePrinter(handle)`
5. `WritePrinter(handle, &escpos_bytes)`
6. `EndPagePrinter(handle)` → `EndDocPrinter(handle)` → `ClosePrinter(handle)`

**Printer enumeration (for the activation window dropdown):** Use `printers` crate (0.2.x, by talesluna) — it wraps `EnumPrinters` from winspool and returns a `Vec<Printer>` with names. Alternatively call `EnumPrintersW` directly via the `windows` crate. The `printers` crate is small enough and simplifies the UI code.

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `printers` | 0.2.x | Enumerate installed Windows printers by name | Thin winspool wrapper; `get_printers()` + `get_default_printer()` covers the activation form dropdown |

**Avoid `winprint` crate:** Focuses on XPS/PDF printing workflows, not raw byte submission to an ESC/POS device. Heavier than needed.

---

### Printing: Raw Bytes via Serial COM Port

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `serialport` | 4.7.x | Open COM port by name (e.g., "COM3"), write raw bytes | Cross-platform; actively maintained at github.com/serialport/serialport-rs; implements `std::io::Write` so sending bytes is trivial |

**Usage pattern:**
```rust
let mut port = serialport::new("COM3", 9600)
    .timeout(Duration::from_millis(2000))
    .open()?;
port.write_all(&escpos_bytes)?;
```

**USB thermal printers on Windows appear in two ways:**
1. As an installed Windows printer (via usbprint.sys) — use the spooler path above
2. As a virtual COM port (via USB-Serial adapter firmware) — use `serialport`

The activation UI should let the user choose which mode via a radio button or by detecting whether the selected name looks like "COMx" vs a printer name.

---

### Pusher Client (WebSocket, Private Channel)

**Recommendation: hand-implement Pusher protocol over `tokio-tungstenite`**

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `tokio-tungstenite` | 0.26.x | Async WebSocket client | Standard, well-maintained; powering most Rust real-time infra |
| `reqwest` | 0.13.x | Channel authentication HTTP POST to Noren backend | Already needed for job fetch/ack; no extra dep |
| `hmac` + `sha2` | latest | HMAC-SHA256 for Pusher auth signature | Pure Rust; `hmac = "0.12"`, `sha2 = "0.10"` from the RustCrypto org |

**Why not use a Pusher crate:**
- `pusher-rs` (chmod77): self-described "still under heavy development", last meaningful activity Sept 2024, API surface is unstable — risky for production
- `pusher` (WillSewell): explicitly marked **[Unsupported]** on its own GitHub README
- `pushers`: HTTP-trigger-only, not a WebSocket subscriber

The Pusher WebSocket protocol is well-documented and straightforward. It reduces to:
1. Connect to `wss://ws-CLUSTER.pusher.com/app/APP_KEY?protocol=7&client=brevly-print&version=0.1`
2. Receive `{"event":"pusher:connection_established","data":...}`
3. Send subscribe: `{"event":"pusher:subscribe","data":{"channel":"private-tenant-X","auth":"APP_KEY:HMAC_SIGNATURE","channel_data":""}}`
4. Generate auth string by POSTing `{socket_id, channel_name}` to Noren's `/api/pusher/auth` endpoint — Noren already implements this (uses Better Auth)
5. Handle incoming `print-job` events; reconnect with exponential backoff on disconnect

This is ~200 lines of well-scoped Rust. **Confidence: HIGH that hand-rolling is the right call.** A thin `PusherClient` struct in `src/pusher.rs` keeps it isolated and testable.

---

### Async Runtime + HTTP Client

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `tokio` | 1.x (stable, use `features = ["full"]` or targeted) | Async runtime for WebSocket connection, HTTP fetches, retry timer | The runtime in the Rust ecosystem; no contest |
| `reqwest` | 0.13.x | HTTP client: fetch ESC/POS bytes, POST ack, POST Pusher auth | Built on hyper+tokio, TLS via rustls by default (no native-tls dependency on Windows), middleware-friendly |

**Note on `reqwest` TLS:** Use the default `rustls-tls` feature (pure Rust TLS). Avoids requiring OpenSSL or the Windows native TLS stack, simplifies cross-compilation and reduces installer size.

---

### Local Persistence (Retry Queue)

**Recommendation: `rusqlite` with bundled SQLite**

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `rusqlite` | 0.32.x | Retry queue: store `(job_id, escpos_bytes, attempt_count, next_retry_at)` rows | SQLite is proven; zero external dependency when bundled; ACID; file is inspectable for debugging |

**Cargo.toml:**
```toml
rusqlite = { version = "0.32", features = ["bundled"] }
```
The `bundled` feature compiles SQLite from source — the resulting binary needs no separate `sqlite3.dll` on Windows.

**Schema is trivially simple:**
```sql
CREATE TABLE IF NOT EXISTS retry_queue (
    job_id      TEXT PRIMARY KEY,
    payload     BLOB NOT NULL,
    attempts    INTEGER NOT NULL DEFAULT 0,
    next_retry  INTEGER NOT NULL  -- Unix timestamp
);
```

**Why not `sled`:** sled 0.34 is pre-1.0 beta, in maintenance mode since 2022, with an unstable on-disk format. Projects actively migrate away from it. For a retry queue that persists across reboots, data integrity matters.

**Why not `redb`:** redb 2.x has a stable file format and is actively maintained (cberner/redb), and would also work. Prefer `rusqlite` because SQL gives richer query expressiveness for `SELECT WHERE next_retry <= NOW() AND attempts < 3` without custom iteration logic.

**Why not plain JSON file:** Race condition risk between crash during write and next read; no atomic updates. Fine for config (see below), wrong for a queue.

**Config file (serial number, printer name, tenant ID):** A plain JSON or TOML file in `%APPDATA%\BrevlyPrint\config.json` via `serde_json` + `dirs` crate. No database needed for 4 fields that change once.

---

### Windows Autostart

**Recommendation: `auto-launch` crate (HKCU Run registry key)**

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `auto-launch` | 0.5.x | Write/remove `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run` entry | Handles Task Manager's `StartupApproved` key so disable-from-Task-Manager works correctly |

**Tradeoff analysis:**

| Approach | Verdict | Reason |
|----------|---------|--------|
| `HKCU` Run registry key via `auto-launch` | **RECOMMENDED** | No elevation needed; user-scoped; visible and controllable in Windows Task Manager Startup tab; appropriate for a per-user tray app |
| Windows Service (`windows-service` crate) | Overkill for this agent | Services run as SYSTEM (no tray access), require admin install/start, and system tray icons cannot be shown from a service session without complex inter-session IPC. Wrong architecture for a tray app. |
| Startup folder | Works, but inferior | Not integrated with Task Manager's enable/disable UI; `auto-launch` chooses this as a fallback if registry fails |
| `HKLM` Run key | Needs admin elevation | Use only if the installer runs elevated and you want system-wide install; `auto-launch` supports this via `WindowsEnableMode` |

**Concrete call:**
```rust
let auto = AutoLaunch::new("BrevlyPrint", std::env::current_exe()?.to_str().unwrap(), false, &[]);
auto.enable()?;
```

---

### Auto-Update

**Recommendation: `velopack`**

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `velopack` (Rust crate) | 0.0.x (mirrors velopack core 1.x) | In-app update check + apply on next boot | Written in Rust internally; official Rust SDK; delta packages; actively maintained; produces the installer too |
| `vpk` CLI tool | 1.2.x | Build releases: packages binary + generates update manifest | Single command to produce a distributable package from compiled binary |

**Why not `self_update`:** `self_update` replaces the binary in-place while it is running — on Windows this fails because the running EXE is locked. It works via a rename trick but requires careful handling. It does NOT produce an installer, only handles the binary swap. For an agent that should update on reboot (not mid-session), this pattern is awkward.

**Velopack approach:**
1. The `velopack` Rust crate checks for updates at startup (HTTP to your S3/Cloudflare update feed)
2. If update is available, it downloads silently in the background
3. On next reboot (or restart), velopack's bootstrapper applies the update before launching the new binary
4. The `vpk pack` command produces both an installer and update packages

**Confidence: MEDIUM.** Velopack is actively developed and the Rust SDK exists, but it's newer than the C#/.NET path and production reports from pure-Rust users are sparse as of mid-2025. If velopack Rust SDK proves immature during Phase implementation, fall back to `self_update` with a scheduled reboot update script. Flag this as needing a spike.

---

### Windows Installer + Code Signing

**Recommendation: Velopack (`vpk`) for packaging, Authenticode OV certificate for signing**

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `vpk` CLI | 1.2.x | Package binary into a `.exe` setup installer (Squirrel-style) | If using Velopack for updates, use `vpk` to build the installer too — consistent toolchain |
| Authenticode OV certificate | N/A | Code signing to reduce SmartScreen warnings | OV (Organization Validation) certs now build SmartScreen reputation identically to EV certs (Microsoft changed policy March 2024); ~$100–400/yr from DigiCert, Sectigo, SSL.com |
| `signtool.exe` (Windows SDK) | N/A | Sign the packaged `.exe` | Standard Windows tool, called in CI after `cargo build --release` |

**Alternative packaging: `cargo-wix`**

`cargo-wix` generates an MSI via WiX toolset. Use this if you need a traditional `.msi` rather than a one-click `.exe` installer. It supports Authenticode signing via the `sign` subcommand. Slightly more configuration overhead than Velopack. Good fallback if Velopack proves problematic.

**SmartScreen reality in 2025:**
- EV certificates no longer bypass SmartScreen (policy change March 2024)
- Both EV and OV certs build reputation through download volume — expect warnings on initial release, clearing after ~hundreds of clean installs
- Mitigations: submit to Microsoft Defender for validation, use a well-known hosting URL, encourage early adopters to click "More info → Run anyway" once
- Do NOT distribute unsigned binaries — Windows will block them entirely on some configurations

---

### Windows Toast Notifications

**Recommendation: `tauri-winrt-notification`**

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `tauri-winrt-notification` | 0.5.x | "Print failed" Windows toast notification | Maintained by tauri-apps; thin WinRT wrapper; tested on Windows 10/11; simple fluent builder API |

**Option comparison:**

| Option | Verdict | Reason |
|--------|---------|--------|
| `tauri-winrt-notification` | **RECOMMENDED** | Actively maintained, thin WinRT binding, no extra runtime dependency |
| `winrt-notification` (allenbenz) | Acceptable alternative | Original crate; same API; less recently updated than tauri's fork |
| `winrt-toast` | Also acceptable | Slightly more modern API; `winrt-toast-reborn` is an active fork |
| Direct `windows` crate WinRT APIs | Works but verbose | 30+ lines to show one toast; not worth the boilerplate vs a wrapper crate |
| `notify-rust` | Avoid on Windows | Linux-native (libnotify/D-Bus); Windows support is secondary and limited |

**Usage pattern:**
```rust
Toast::new(Toast::POWERSHELL_APP_ID)
    .title("Brevly Print")
    .text1("Falha ao imprimir — impressora inacessível")
    .show()?;
```

Note: The `app_id` must match a registered Start Menu entry for the notification to persist correctly; velopack's installer creates this entry automatically. If using a custom installer, register the app ID in the registry under `SOFTWARE\Classes\AppUserModelId\`.

---

## Complete Dependency Summary

```toml
[dependencies]
# Tray + event loop
tray-icon    = "0.21"
tao          = "0.35"
muda         = "0.15"       # context menu (pulled by tray-icon, re-export for clarity)

# Activation window
egui         = "0.31"
eframe       = { version = "0.31", features = ["default_fonts"] }

# Windows printing (spooler path)
[dependencies.windows]
version  = "0.62"
features = ["Win32_Graphics_Printing", "Win32_Foundation"]

# Printer enumeration
printers = "0.2"

# Serial port printing
serialport = "4.7"

# Async runtime + HTTP
tokio       = { version = "1", features = ["full"] }
reqwest     = { version = "0.13", default-features = false, features = ["rustls-tls", "json"] }

# WebSocket (Pusher)
tokio-tungstenite = { version = "0.26", features = ["rustls-tls-webpki-roots"] }

# Pusher channel auth HMAC
hmac = "0.12"
sha2 = "0.10"

# Local persistence
rusqlite = { version = "0.32", features = ["bundled"] }

# Config serialization
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"
dirs        = "5"

# Autostart
auto-launch = "0.5"

# Auto-update
velopack    = "0.0"         # check latest on crates.io at implementation time

# Windows notifications
tauri-winrt-notification = "0.5"

# Error handling
anyhow = "1"

[build-dependencies]
# If using cargo-wix fallback
# cargo-wix handled externally as a cargo subcommand
```

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| Event loop | `tao` | `winit` | winit 0.30+ redesigned API; no built-in menu support; `tray-icon` examples use `tao` |
| System tray | `tray-icon` | `trayicon` (Ciantic) | Unmaintained since 2022 |
| Activation GUI | `egui` | `native-windows-gui` | NWG is in permanent maintenance mode, author has moved on |
| Printer spooler | `windows` crate direct | `winprint` | `winprint` focuses on XPS/PDF, not raw byte submission |
| Pusher client | hand-rolled over `tokio-tungstenite` | `pusher-rs`, `pusher` | `pusher-rs` under development; `pusher` explicitly unsupported |
| Local store | `rusqlite` bundled | `sled`, `redb`, JSON | `sled` abandoned; `redb` works but SQL is more expressive for queue queries; plain JSON has no atomic writes |
| Autostart | `auto-launch` (HKCU) | Windows Service | Services can't show tray icons; require elevation; wrong architecture |
| Auto-update | `velopack` | `self_update` | `self_update` can't replace locked EXE on Windows cleanly; no installer story |
| Notifications | `tauri-winrt-notification` | `notify-rust` | `notify-rust` Windows support is limited |

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Tray + event loop | HIGH | `tray-icon` + `tao` confirmed by tauri-apps maintained crates with active releases in 2025 |
| Activation GUI | MEDIUM-HIGH | `egui` is well-documented; the thread interaction with `tao` needs a spike to validate |
| Windows print spooler | HIGH | Win32 API is stable; `windows` crate `Win32_Graphics_Printing` module confirmed by official docs.rs |
| Serial port | HIGH | `serialport` 4.x is stable, cross-platform, actively maintained |
| Pusher (hand-rolled) | HIGH | Protocol spec is public; `tokio-tungstenite` is proven; ~200 LoC scope |
| Async runtime | HIGH | `tokio` 1.x + `reqwest` 0.13 are de facto standard |
| Local persistence | HIGH | `rusqlite` bundled is widely used in production Rust apps |
| Autostart | HIGH | `auto-launch` confirmed active, handles Task Manager integration |
| Auto-update | MEDIUM | Velopack Rust SDK is newer; recommend a spike in Phase 1 |
| Installer + signing | MEDIUM | SmartScreen policy confirmed changed March 2024; EV no longer instant-clean — plan reputation-building time |
| Toast notifications | MEDIUM-HIGH | `tauri-winrt-notification` confirmed working on Win10/11; app_id registration is a gotcha |

---

## Sources

- [tray-icon crates.io](https://crates.io/crates/tray-icon)
- [tauri-apps/tray-icon GitHub](https://github.com/tauri-apps/tray-icon)
- [tao crates.io](https://crates.io/crates/tao)
- [native-windows-gui crates.io](https://crates.io/crates/native-windows-gui) — confirms "3rd and final version"
- [egui GitHub](https://github.com/emilk/egui)
- [windows crate Win32::Graphics::Printing docs](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Graphics/Printing/)
- [Microsoft Learn: Send raw data to printer (Win32 API)](https://learn.microsoft.com/en-us/previous-versions/troubleshoot/windows/win32/win32-raw-data-to-printer)
- [printers crates.io / rust-printers GitHub](https://github.com/talesluna/rust-printers)
- [serialport crates.io](https://crates.io/crates/serialport)
- [tokio-tungstenite GitHub](https://github.com/snapview/tokio-tungstenite)
- [Pusher Channels WebSocket Protocol](https://pusher.com/docs/channels/library_auth_reference/pusher-websockets-protocol/)
- [pusher-http-rust GitHub — Unsupported](https://github.com/WillSewell/pusher-http-rust)
- [pusher-rs GitHub](https://github.com/chmod77/pusher-rs)
- [reqwest crates.io](https://crates.io/crates/reqwest)
- [rusqlite crates.io](https://crates.io/crates/rusqlite)
- [sled GitHub — beta/maintenance](https://github.com/spacejam/sled)
- [redb crates.io](https://crates.io/crates/redb)
- [auto-launch crates.io](https://crates.io/crates/auto-launch)
- [velopack.io](https://velopack.io/)
- [velopack crates.io](https://crates.io/crates/velopack)
- [cargo-wix crates.io](https://crates.io/crates/cargo-wix)
- [SmartScreen reputation — Microsoft Learn](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
- [EV certificate SmartScreen change March 2024](https://learn.microsoft.com/en-us/answers/questions/417016/reputation-with-ov-certificates-and-are-ev-certifi)
- [tauri-winrt-notification crates.io](https://crates.io/crates/tauri-winrt-notification)
- [winrt-toast lib.rs](https://lib.rs/crates/winrt-toast)
- [windows-rs GitHub (Microsoft)](https://github.com/microsoft/windows-rs)
