<!-- GSD:project-start source:PROJECT.md -->
## Project

**Brevly Print**

Brevly Print é um agente nativo de impressão para Windows que substitui o QZ Tray no
Noren (SaaS de gestão de restaurantes). O dono do restaurante instala uma vez, ativa com
um serial number e seleciona a impressora térmica — a partir daí o agente roda invisível
na bandeja do sistema e imprime comandas e cupons automaticamente quando eventos chegam
do Noren, sem que o funcionário do caixa precise interagir com nada.

**Core Value:** Quando um evento de impressão chega do Noren, a comanda/cupom correto sai na impressora
térmica em menos de 1 segundo, de forma confiável e sem intervenção humana — nenhuma
comanda perdida, mesmo com impressora ou internet fora do ar.

### Constraints

- **Tech stack**: Rust nativo — binário único enxuto, `tray-icon` (bandeja),
  `native-windows-gui` (tela de ativação), `serialport`/spooler USB para impressão. Sem
  webview. Escolhido por menor footprint, confiabilidade always-on e menor superfície de revisão.
- **Plataforma**: Windows apenas (v1). Impressoras USB e serial apenas.
- **Latência**: comanda na impressora em < 1 segundo após o evento.
- **Confiabilidade**: nenhuma comanda perdida — retry local (impressora offline) + fila
  server-side no Noren (agente offline/internet caiu).
- **Transporte**: Pusher para wakeup do evento + HTTP autenticado para payload/ack. Payload
  não vai pelo Pusher (limite ~10KB; cupom de fechamento pode estourar).
- **Renderização**: bytes ESC/POS gerados pelo Noren (spooler burro no agente) — fonte única
  de verdade dos templates, evita duplicar/portar layout e QR para Rust.
- **Ativação/licença**: serial gerado e validado pelo backend do Noren (serial → tenant).
- **Operação**: usuário-alvo é o dono/gerente (instala 1×); o caixa nunca interage.
- **Modo de trabalho**: desenvolvimento 100% via GSD; o dono do projeto apenas revisa.
<!-- GSD:project-end -->

<!-- GSD:stack-start source:research/STACK.md -->
## Technology Stack

## Recommended Stack
### System Tray + Event Loop
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `tray-icon` | 0.21.x (latest on crates.io as of research date) | System tray icon with color-state, tooltip, right-click menu | Maintained by tauri-apps; works without a webview; first-class Windows support via win32 event loop |
| `tao` | 0.35.x | Win32 event loop that pumps tray-icon events | Also maintained by tauri-apps; the recommended pairing with `tray-icon` per the crate's own examples |
### Activation Window (Single-Use GUI)
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `egui` | 0.31.x | Immediate-mode GUI widgets (text input, combo box, button) | Pure Rust, no webview, ships its own renderer (wgpu/OpenGL), actively maintained |
| `eframe` | 0.31.x | Native window host for egui on Windows | Official egui native backend; creates a real Win32 window |
| Option | Verdict | Reason |
|--------|---------|--------|
| `egui` + `eframe` | **RECOMMENDED** | Pure Rust, tiny binary delta, actively maintained (Emil Ernerfeldt), one-window use is trivially easy |
| `native-windows-gui` (NWG) | Not recommended | Declared "3rd and final version", in maintenance mode, future development moved elsewhere — fine today but a liability in 2–3 years. Also requires hand-writing Win32 dialog layout. |
| Raw `windows` crate Win32 dialogs | Avoid | Massive boilerplate for a single activation form; no benefit over egui for this use case |
| Tauri | Hard no | Pulls WebView2 runtime — contradicts the constraint "no webview" |
### Printing: Raw Bytes to Windows Printer (USB via Spooler)
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `windows` | 0.62.x | Win32 print spooler API: `OpenPrinterW`, `StartDocPrinterW`, `WritePrinter`, `EndDocPrinter` | Official Microsoft-maintained bindings; all winspool functions are available under `Win32::Graphics::Printing` feature flag |
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `printers` | 0.2.x | Enumerate installed Windows printers by name | Thin winspool wrapper; `get_printers()` + `get_default_printer()` covers the activation form dropdown |
### Printing: Raw Bytes via Serial COM Port
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `serialport` | 4.7.x | Open COM port by name (e.g., "COM3"), write raw bytes | Cross-platform; actively maintained at github.com/serialport/serialport-rs; implements `std::io::Write` so sending bytes is trivial |
### Pusher Client (WebSocket, Private Channel)
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `tokio-tungstenite` | 0.26.x | Async WebSocket client | Standard, well-maintained; powering most Rust real-time infra |
| `reqwest` | 0.13.x | Channel authentication HTTP POST to Noren backend | Already needed for job fetch/ack; no extra dep |
| `hmac` + `sha2` | latest | HMAC-SHA256 for Pusher auth signature | Pure Rust; `hmac = "0.12"`, `sha2 = "0.10"` from the RustCrypto org |
- `pusher-rs` (chmod77): self-described "still under heavy development", last meaningful activity Sept 2024, API surface is unstable — risky for production
- `pusher` (WillSewell): explicitly marked **[Unsupported]** on its own GitHub README
- `pushers`: HTTP-trigger-only, not a WebSocket subscriber
### Async Runtime + HTTP Client
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `tokio` | 1.x (stable, use `features = ["full"]` or targeted) | Async runtime for WebSocket connection, HTTP fetches, retry timer | The runtime in the Rust ecosystem; no contest |
| `reqwest` | 0.13.x | HTTP client: fetch ESC/POS bytes, POST ack, POST Pusher auth | Built on hyper+tokio, TLS via rustls by default (no native-tls dependency on Windows), middleware-friendly |
### Local Persistence (Retry Queue)
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `rusqlite` | 0.32.x | Retry queue: store `(job_id, escpos_bytes, attempt_count, next_retry_at)` rows | SQLite is proven; zero external dependency when bundled; ACID; file is inspectable for debugging |
### Windows Autostart
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `auto-launch` | 0.5.x | Write/remove `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run` entry | Handles Task Manager's `StartupApproved` key so disable-from-Task-Manager works correctly |
| Approach | Verdict | Reason |
|----------|---------|--------|
| `HKCU` Run registry key via `auto-launch` | **RECOMMENDED** | No elevation needed; user-scoped; visible and controllable in Windows Task Manager Startup tab; appropriate for a per-user tray app |
| Windows Service (`windows-service` crate) | Overkill for this agent | Services run as SYSTEM (no tray access), require admin install/start, and system tray icons cannot be shown from a service session without complex inter-session IPC. Wrong architecture for a tray app. |
| Startup folder | Works, but inferior | Not integrated with Task Manager's enable/disable UI; `auto-launch` chooses this as a fallback if registry fails |
| `HKLM` Run key | Needs admin elevation | Use only if the installer runs elevated and you want system-wide install; `auto-launch` supports this via `WindowsEnableMode` |
### Auto-Update
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `velopack` (Rust crate) | 0.0.x (mirrors velopack core 1.x) | In-app update check + apply on next boot | Written in Rust internally; official Rust SDK; delta packages; actively maintained; produces the installer too |
| `vpk` CLI tool | 1.2.x | Build releases: packages binary + generates update manifest | Single command to produce a distributable package from compiled binary |
### Windows Installer + Code Signing
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `vpk` CLI | 1.2.x | Package binary into a `.exe` setup installer (Squirrel-style) | If using Velopack for updates, use `vpk` to build the installer too — consistent toolchain |
| Authenticode OV certificate | N/A | Code signing to reduce SmartScreen warnings | OV (Organization Validation) certs now build SmartScreen reputation identically to EV certs (Microsoft changed policy March 2024); ~$100–400/yr from DigiCert, Sectigo, SSL.com |
| `signtool.exe` (Windows SDK) | N/A | Sign the packaged `.exe` | Standard Windows tool, called in CI after `cargo build --release` |
- EV certificates no longer bypass SmartScreen (policy change March 2024)
- Both EV and OV certs build reputation through download volume — expect warnings on initial release, clearing after ~hundreds of clean installs
- Mitigations: submit to Microsoft Defender for validation, use a well-known hosting URL, encourage early adopters to click "More info → Run anyway" once
- Do NOT distribute unsigned binaries — Windows will block them entirely on some configurations
### Windows Toast Notifications
| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `tauri-winrt-notification` | 0.5.x | "Print failed" Windows toast notification | Maintained by tauri-apps; thin WinRT wrapper; tested on Windows 10/11; simple fluent builder API |
| Option | Verdict | Reason |
|--------|---------|--------|
| `tauri-winrt-notification` | **RECOMMENDED** | Actively maintained, thin WinRT binding, no extra runtime dependency |
| `winrt-notification` (allenbenz) | Acceptable alternative | Original crate; same API; less recently updated than tauri's fork |
| `winrt-toast` | Also acceptable | Slightly more modern API; `winrt-toast-reborn` is an active fork |
| Direct `windows` crate WinRT APIs | Works but verbose | 30+ lines to show one toast; not worth the boilerplate vs a wrapper crate |
| `notify-rust` | Avoid on Windows | Linux-native (libnotify/D-Bus); Windows support is secondary and limited |
## Complete Dependency Summary
# Tray + event loop
# Activation window
# Windows printing (spooler path)
# Printer enumeration
# Serial port printing
# Async runtime + HTTP
# WebSocket (Pusher)
# Pusher channel auth HMAC
# Local persistence
# Config serialization
# Autostart
# Auto-update
# Windows notifications
# Error handling
# If using cargo-wix fallback
# cargo-wix handled externally as a cargo subcommand
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
<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->
## Conventions

Conventions not yet established. Will populate as patterns emerge during development.
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->
## Architecture

Architecture not yet mapped. Follow existing patterns found in the codebase.
<!-- GSD:architecture-end -->

<!-- GSD:skills-start source:skills/ -->
## Project Skills

No project skills found. Add skills to any of: `.claude/skills/`, `.agents/skills/`, `.cursor/skills/`, `.github/skills/`, or `.codex/skills/` with a `SKILL.md` index file.
<!-- GSD:skills-end -->

<!-- GSD:workflow-start source:GSD defaults -->
## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:
- `/gsd-quick` for small fixes, doc updates, and ad-hoc tasks
- `/gsd-debug` for investigation and bug fixing
- `/gsd-execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->



<!-- GSD:profile-start -->
## Developer Profile

> Profile not yet configured. Run `/gsd-profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->
