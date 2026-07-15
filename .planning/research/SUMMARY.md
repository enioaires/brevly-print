# Research Summary

**Project:** Brevly Print — Native Windows Thermal Print Agent
**Domain:** Unattended background agent (ESC/POS thermal printing + Pusher WebSocket + Windows deployment)
**Researched:** 2026-07-15
**Confidence:** MEDIUM-HIGH

---

## Executive Summary

Brevly Print is a Rust native Windows agent that replaces QZ Tray as the thermal-printer bridge for the Noren restaurant SaaS. The strategic forcing function is Chrome 147's Local Network Access (LNA) restrictions (April 2026), which broke `ws://localhost:8181` connections for all QZ Tray deployments requiring open browser tabs. Brevly Print eliminates the browser dependency entirely: a Pusher push event wakes the agent, an authenticated HTTP call fetches the pre-rendered ESC/POS bytes from Noren's server, and the raw bytes are written directly via the Windows print spooler. The agent runs invisibly as a tray icon, survives reboots, and has no user-interaction surface beyond a one-time activation window.

The recommended approach is a two-thread-domain architecture: Win32 main thread owns the tray icon and message pump (`tray-icon` + `tao`), while a Tokio thread pool handles all async I/O (Pusher WebSocket, HTTP job fetch, ack, retry). The agent is intentionally a dumb spooler — Noren renders ESC/POS server-side (migrating from existing client-side TypeScript builders in `ticket.ts`), keeping all template logic, character encoding, and QR generation in one place and out of Rust. Local persistence is a bundled SQLite database (`rusqlite`) used solely for dedup tracking and retry-queue state; the server-side pending queue on Noren is the source of truth for offline resilience.

The dominant risks are: (1) the `eframe` + `tao` event-loop ownership conflict on Windows, which must be resolved in a Phase 1 spike before any GUI code is written; (2) the requirement for paired Noren backend changes (a new `/api/agent/` contract) that block nearly every integration feature — this is a first-class project dependency, not a background concern; and (3) Windows deployment hazards — SmartScreen reputation building, antivirus false positives, and auto-update mechanics — that require Authenticode signing from the very first release and Velopack as the update bootstrapper.

---

## Reconciled Disagreements

Four specific conflicts arose between dimension researchers. These are resolved here with a single recommended answer each.

### 1. Activation Window GUI: egui/eframe vs native-windows-gui

**Resolution: egui (via raw egui-wgpu integration inside the tao event loop) — NOT `eframe::run_native()`.**

STACK.md correctly recommended egui/eframe over NWG (feature-frozen). ARCHITECTURE.md incorrectly assumed `native-windows-gui` was viable. The critical constraint: both `eframe::run_native()` and `tao::EventLoop::run()` block the calling thread and each owns a Win32 message loop — two event loops cannot coexist in one process on Windows, and `EventLoop` is `!Send` so it cannot be moved to a background thread.

The correct integration is: use `egui` directly with `egui-wgpu` (or `egui-glow`) as the renderer, create the activation window via `tao::window::WindowBuilder`, and drive the egui paint loop from within the existing `tao::EventLoop::run()` callback. `eframe::run_native()` is explicitly disallowed. An acceptable alternative is a separate short-lived subprocess for the setup window communicating results via a temp file or named pipe.

**Phase 1 spike is mandatory:** prove the `tao` + raw `egui` rendering thread model works before writing any activation-window code. This is the single highest-risk item gating all GUI work.

### 2. Auto-Update: velopack vs self_update

**Resolution: Velopack is primary; self_update + RunOnce registry key is the documented fallback.**

STACK.md recommended Velopack; ARCHITECTURE.md cited `self_update`. The distinction matters on Windows because a running EXE is file-locked — `self_update`'s in-place replacement fails with `ERROR_SHARING_VIOLATION`. Velopack solves this correctly by using a bootstrapper process (`Update.exe`) that applies the update after the agent exits, never touching the locked binary mid-flight.

If the Velopack Rust SDK proves immature during Phase 7 implementation (MEDIUM confidence per STACK.md), the fallback is: download the new binary to `%APPDATA%\BrevlyPrint\update\`, write a `RunOnce` registry key to replace it on next boot, and record the pending update in SQLite. This avoids all file-lock issues at the cost of no delta packages.

### 3. USB Print Path: windows crate WritePrinter vs escpos crate CreateFile/WriteFile

**Resolution: Primary path is `windows` crate `WritePrinter` (winspool RAW datatype) for printers appearing as installed Windows printers. The `escpos`/`CreateFile`/`WriteFile` approach is not used in v1.**

The Epson TM-T20X (primary target) uses usbprint.sys and appears as an installed Windows printer — `WritePrinter` is correct and simpler. Cheaper Chinese 80mm clones may use USB-Serial bridges and present as `COMx`, in which case `serialport` (not `escpos`/`CreateFile`) handles them via the serial path. The `escpos` crate for direct USB raw access via `\\.\USB001` requires a Zadig/WinUSB driver swap — do not implement in v1.

`PrintWorker` implements two paths gated on `printer_type` config: `windows_printer` (winspool `WritePrinter`) and `serial_port` (`serialport` crate). The activation window enumerates both sources — installed printers via `printers` crate and COM ports via `serialport::available_ports()`.

### 4. Pusher Client: hand-rolled over tokio-tungstenite (confirmed — no maintained crate)

**Resolution: Hand-roll the Pusher WebSocket protocol over `tokio-tungstenite`. No Pusher crate.**

Both `pusher` (WillSewell — explicitly unsupported) and `pusher-rs` (chmod77 — unstable API, last meaningful activity Sept 2024) are disqualified. The protocol is ~200 lines of well-scoped Rust in `src/pusher.rs`.

Critical implementation details:
- **Re-auth per socket_id:** Every reconnect yields a new `socket_id` from `pusher:connection_established`. Auth string is `HMAC_SHA256(appSecret, "${socketId}:${channelName}")` — tied to that session. Never cache auth strings. Always re-POST `/api/agent/pusher/auth` with the fresh `socket_id` before sending `pusher:subscribe`.
- **Silent-reconnect mitigation (ping/pong):** After >5 minutes offline, Pusher can close the TCP connection without firing `onclose`, leaving the socket in a zombie state. Implement application-level ping/pong: send `{"event":"pusher:ping","data":{}}` every 30 seconds, expect `pusher:pong` within 10 seconds via `tokio::time::timeout`. On timeout, close explicitly and trigger reconnect with exponential backoff (1s → 2s → 4s → max 60s).
- **On every reconnect:** re-auth → re-subscribe → immediately run `PendingPoller` to drain any missed jobs.

---

## Key Findings

### Recommended Stack

All crates verified against crates.io and docs.rs as of 2026-07-15. The only areas of lower confidence are Velopack Rust SDK maturity (MEDIUM) and SmartScreen reputation timing (MEDIUM).

**Core technologies:**

- `tray-icon` 0.21 + `tao` 0.35 — system tray icon and Win32 event loop; canonical tauri-apps pairing; `tao` pumps the loop `tray-icon` requires on the main thread
- `egui` 0.31 (raw, NOT via `eframe::run_native()`) — activation window GUI; pure Rust; compatible with `tao` when integrated directly via `egui-wgpu`
- `windows` crate 0.62 (`Win32_Graphics_Printing`) — USB print path; `OpenPrinterW` / `StartDocPrinterW` / `WritePrinter` / `EndDocPrinterW` with `pDatatype = "RAW"`
- `printers` 0.2 — printer enumeration in activation dropdown; thin `EnumPrintersW` wrapper
- `serialport` 4.7 — serial/COM port print path; covers USB-Serial bridge printers
- `tokio` 1.x + `reqwest` 0.13 (rustls-tls) — async runtime and HTTP client; rustls avoids native TLS dependency
- `tokio-tungstenite` 0.26 + `hmac`/`sha2` — hand-rolled Pusher WebSocket client; no maintained Pusher crate exists
- `rusqlite` 0.32 (bundled) — job dedup, retry-queue, config; bundled SQLite; zero external dll; ACID transactions survive reboots
- `auto-launch` 0.5 — HKCU Run registry autostart; correctly handles `StartupApproved` key for Task Manager integration
- `velopack` 0.0 + `vpk` CLI — auto-update and installer; bootstrapper update pattern solves Windows file-lock problem
- `tauri-winrt-notification` 0.5 — Windows toast notifications for print failures
- `windows-dpapi` crate — DPAPI Scope::User encryption for agentToken at rest

**Explicit prohibitions:**
- Do NOT use `eframe::run_native()` — only raw `egui` rendering into `tao` window
- Do NOT use `native-windows-gui` — feature-frozen, maintenance-only
- Do NOT use `trayicon` (Ciantic) — unmaintained since 2022
- Do NOT use `pusher-rs` or `pusher` crates — both disqualified
- `reqwest` must use `default-features = false, features = ["rustls-tls", "json"]`

### Expected Features

**Must have (table stakes) — v1 MVP:**
- Autostart with Windows (HKCU Run via `auto-launch`)
- Invisible operation — no UI except tray icon during normal operation
- System tray icon: green = connected, red = problem
- One-time activation screen: serial input + printer dropdown + Save button
- Printer enumeration (Windows printers + COM ports in combined dropdown)
- Print comanda de pedido, comanda do entregador, cupom de fechamento (raw ESC/POS from Noren)
- Near-instant print (<1s: Pusher wakeup ~100-200ms + HTTP fetch ~100ms + WritePrinter ~50ms)
- Automatic retry: 3× with 30s interval on printer failure
- Windows toast notification when retries exhausted (plain language)
- Server-side queue pull on Pusher reconnect (`GET /api/agent/jobs/pending`)
- Serial-based authentication (serial → tenant binding via Noren backend)
- HTTP delivery ack with confirmation (`POST /api/agent/jobs/{jobId}/ack`)

**Should have (differentiators) — v1.1:**
- Per-event-type enable/disable (`print_order`, `print_dispatch`, `print_closing`) — requires Noren dashboard UI; build both sides together
- Agent heartbeat every 60s — enables Noren dashboard "printer online/offline" indicator; prerequisite for AV quarantine detection
- Silent auto-update on next restart via Velopack — requires update hosting endpoint

**Defer (v2+):**
- Printer status reporting via ESC/POS DLE EOT — HIGH complexity; unreliable via USB spooler
- Multiple printers per agent
- Network printer support
- Cash drawer kick, print history UI in agent

### Architecture Approach

The agent splits into two thread domains communicating via bounded channels. The Win32 main thread owns the `tray-icon` message loop and all Win32 window handles. The Tokio thread pool handles all async I/O. Cross-thread communication: `std::sync::mpsc::SyncSender<TrayUpdate>` from tokio to main (non-blocking); `tokio::sync::oneshot::Sender` from activation GUI to async runtime. All print jobs flow through a typed pipeline: EventListener → JobQueue → JobFetcher → PrintWorker → (AckSender | RetryScheduler).

**Major components:**

1. **Win32 Main Thread** — `tray-icon` + `tao` event loop; egui activation window (first-run only); receives `TrayUpdate` from tokio via sync channel
2. **EventListener** — hand-rolled Pusher WebSocket client over `tokio-tungstenite`; ping/pong health check; triggers `PendingPoller` on reconnect
3. **JobFetcher** — authenticated `GET /api/agent/jobs/{jobId}/bytes`; dedup via `INSERT OR IGNORE INTO printed_jobs`; returns `Vec<u8>` to PrintWorker
4. **PrintWorker** — winspool path (`WritePrinter` RAW) or `serialport` path based on `printer_type` config; writes `status = 'done'` to SQLite before handing off to AckSender
5. **RetryScheduler** — 3× / 30s retry via `tokio::time::sleep`; updates SQLite attempt count; triggers tray red + toast on exhaustion
6. **AckSender** — `POST /api/agent/jobs/{jobId}/ack` only after `done` written to SQLite; 409 Already Acked treated as success
7. **ConfigStore** — SQLite `state.db`: `config` table, `printed_jobs` table, `retry_queue` table
8. **CredentialStore** — DPAPI Scope::User encrypted `credential.bin`; graceful decryption failure → re-activation flow
9. **UpdateChecker** — polls `GET /api/agent/version` nightly; downloads and schedules via Velopack; applies on next reboot

**SQLite is not a local job queue** — it is a dedup tracker and retry coordinator. Noren's `agent_print_jobs` table is the authoritative job queue.

### Critical Pitfalls

1. **eframe + tao event loop conflict (C2)** — `eframe::run_native()` and `tao::EventLoop::run()` both own the Win32 message loop; cannot coexist in one process. Resolve in Phase 1 spike before any GUI code.

2. **RAW datatype not set in WritePrinter (C1)** — omitting `pDatatype = "RAW"` in `DOC_INFO_1W` routes ESC/POS bytes through the XPS driver pipeline; `WritePrinter` returns success while printer emits garbage. Always set `"RAW"`. Add test-print in activation window.

3. **Pusher zombie connection after >5 min offline (C5)** — TCP closed silently at NAT/firewall; `readyState` stays `OPEN`; no `onclose` fires; agent appears green while missing all events. Mandatory ping/pong every 30s from day one.

4. **Pusher socket_id changes on reconnect; auth must be re-requested (M4)** — cached auth strings rejected post-reconnect; channel subscription silently fails. Re-POST auth with new `socket_id` on every reconnect.

5. **Ack before print confirmed = silently lost ticket (C4)** — Noren removes job from pending on ack; crash between ack and printer write loses the job permanently. Order: PrintWorker success → write `done` to SQLite → AckSender → POST ack.

6. **In-memory dedup lost on crash = reprints on reconnect (C3)** — `HashSet<String>` cleared on any restart; PendingPoller re-delivers already-printed jobs. SQLite `printed_jobs` with `done` status is the only correct dedup fence. `INSERT OR IGNORE` on PRIMARY KEY for atomicity.

7. **Auto-update replaces locked running EXE (M6)** — `self_update` in-place replacement fails with `ERROR_SHARING_VIOLATION` on Windows. Use Velopack bootstrapper (update after agent exits). Fallback: temp download + `RunOnce` key.

8. **DPAPI credential lost after Windows reinstall (M7)** — new SID = new DPAPI master key = unreadable `credential.bin`. Activation endpoint must support re-activation (invalidate old token, issue new). Agent must catch DPAPI failure, clear credentials, re-enter activation flow.

9. **SmartScreen + AV false positives (M5, M9)** — Rust binary with network + registry + printer access matches RAT behavioral signatures. Authenticode OV signing mandatory from first release. VirusTotal scan in CI. Heartbeat enables Noren dashboard visibility if agent is silently quarantined.

---

## Agent ↔ Noren API Contract (First-Class Dependency)

**This is the most important cross-cutting concern.** The agent cannot do anything useful without Noren backend changes. These live in a separate repo (`~/repos/brevly/noren`) and are a prerequisite that must be tracked as a parallel workstream.

Every integration feature is BLOCKED until the corresponding Noren API exists.

### Required Noren Backend Changes

| Noren Change | Blocks Agent Feature | Priority |
|---|---|---|
| Server-side ESC/POS rendering (migrate `buildTicket`, `buildDespachoTicket`, `buildClosingTicket` from `ticket.ts` to SvelteKit server) | Everything — agent receives no bytes without this | P0 |
| `agent_print_jobs` + `agent_serials` tables in DB schema | Everything — no serial auth, no job queue | P0 |
| `POST /api/agent/activate` — serial validation, agentToken issuance, re-activation support | Activation flow (Phase 2) | P0 |
| `POST /api/agent/pusher/auth` — HMAC auth with tenant-channel validation (hard 403 if channel != agent's tenantId) | Pusher subscription (Phase 4) | P0 |
| Pusher event emission on order/dispatch/close transitions (`{jobId, type}` lightweight payload) | Receiving print events (Phase 4) | P0 |
| `GET /api/agent/jobs/{jobId}/bytes` — return base64 ESC/POS for specific job | Job fetch (Phase 5) | P0 |
| `POST /api/agent/jobs/{jobId}/ack` — mark job printed; idempotent (409 on repeat, never 5xx) | Delivery ack + dedup (Phase 5) | P0 |
| `GET /api/agent/jobs/pending` — unacked jobs for tenant, sorted `createdAt ASC`, max 100 | Offline resilience / reconnect pull (Phase 6) | P1 |
| `POST /api/agent/heartbeat` — record agent last-seen, printer status | Noren dashboard visibility; AV quarantine detection | P1 |
| `GET /api/agent/version` — latest version + downloadUrl + SHA256 | Auto-update (Phase 7) | P2 |
| `enabled_types` in activation response | Per-type enable/disable (v1.1) | P1 |

**Encoding risk:** Noren's existing `ticket.ts` uses ISO-8859-1. The server-side migration must preserve this exactly — Node.js string handling defaults to UTF-8; explicit Buffer encoding is required. Must be validated with real printer output for `ã`, `ç`, `é`, `ó`, `ú`, `Ç`.

**Channel auth security:** The `/api/agent/pusher/auth` handler must validate `channel_name === private-tenant-${tenantId}-print` where `tenantId` comes from the authenticated agentToken. Not a log-only check — a hard 403 return. Prevents cross-tenant event leakage (C6).

---

## Implications for Roadmap

Research suggests 7 phases following the architecture's component dependency graph. Phases 1–5 are the critical path and must be sequential. Phase 6 (Resilience) can be developed in parallel with Phase 5. Phase 7 (Auto-Update) is independent after Phase 3.

### Phase 1: Foundation + Thread Model Spike

**Rationale:** All other work depends on the runtime architecture being proven. The `tao` + raw `egui` integration is the highest-risk unknown in the entire project — it must be resolved before writing any GUI code.

**Delivers:**
- Thread model spike: `tao` event loop + egui rendering integration validated
- SQLite `state.db` schema with all three tables (`config`, `printed_jobs`, `retry_queue`)
- `CredentialStore` with DPAPI Scope::User encrypt/decrypt + graceful failure path
- `%APPDATA%\BrevlyPrint\` directory initialization (`create_dir_all`)
- Cargo.toml with full dependency set, Windows target confirmed

**Addresses:** C2 (thread model spike), m2 (SQLite directory init), M7 (DPAPI failure recovery)

**Research flag:** REQUIRED SPIKE — prove `tao` + raw `egui` or subprocess approach before committing. If egui embedding fails, the subprocess approach changes Phase 2 significantly.

### Phase 2: Activation Flow

**Rationale:** No feature is testable without a bound serial. Activation issues the agentToken required for Pusher auth. Must precede all network integration.

**Delivers:**
- Activation window (egui via approach validated in Phase 1): serial input + printer dropdown (Windows printers + COM ports combined) + Save button
- `POST /api/agent/activate` call with instant visual feedback
- DPAPI token storage in `credential.bin`
- Config persistence to SQLite: `printer_name`, `printer_type`, `tenant_id`, `enabled_types`
- HKCU Run autostart via `auto-launch` crate; startup health check via `auto_launch.is_enabled()`
- Test-print button sending `ESC @` reset + `GS V` cut to validate RAW bytes reach printer

**Addresses:** C1 (test-print validates RAW datatype), M1 (dual enumeration), M8 (auto-launch handles StartupApproved)

**Blocked by:** Noren `POST /api/agent/activate` + `agent_serials` table

### Phase 3: Tray + Runtime Bridge + First Distributable

**Rationale:** Establishes the always-on invariant and produces the first signed installer. Signing must be in place before any network-facing code ships.

**Delivers:**
- Win32 main thread with `tao::EventLoop` + `tray-icon` (green/red icon)
- Tokio multi-thread runtime spawned on a dedicated OS thread
- `std::sync::mpsc::SyncSender<TrayUpdate>` bridge (tokio → tray, non-blocking `try_send`)
- `Arc<AppState>` initialization (loads credential, config, SQLite)
- Startup recovery: re-enqueue rows with `status = 'printing'` at boot
- First signed `.exe` installer built with `vpk` CLI + Authenticode OV certificate
- AppUserModelId registry entry (required for toasts to show correct app name)
- VirusTotal scan step in CI pipeline

**Addresses:** M5 (sign from first release), M9 (VirusTotal in CI), m1 (AppUserModelId registration)

**Research flag:** Installer toolchain (`vpk`) and Authenticode signing pipeline setup. Plan for 2–6 weeks SmartScreen warning period at launch.

### Phase 4: Pusher Integration

**Rationale:** Pusher is the event delivery mechanism. Nothing prints without it. Channel auth is the trust boundary.

**Delivers:**
- Hand-rolled Pusher WebSocket client over `tokio-tungstenite` in `src/pusher.rs`
- HMAC-SHA256 channel auth — re-POST on every reconnect with fresh `socket_id`
- Ping/pong health check: 30s interval, 10s pong timeout via `tokio::time::timeout`
- Exponential backoff reconnect (1s → 2s → 4s → max 60s)
- `TrayUpdate::Reconnecting` (yellow) state during backoff
- `PendingPoller` trigger on every successful reconnect
- `PrintEvent { jobId, type }` → `JobQueue` channel

**Addresses:** C5 (zombie connection — ping/pong mandatory from day one), M4 (socket_id re-auth on reconnect), C6 (flag for Noren: validate channel name against agentToken tenantId), m5 (per-tenant channel scoping)

**Blocked by:** Noren `POST /api/agent/pusher/auth`; Pusher event emission on order/dispatch/close

**Research flag:** Integration testing against real Pusher infrastructure; >5min outage simulation; reconnect test must pass before shipping.

### Phase 5: Job Pipeline (Core Print Path)

**Rationale:** End-to-end critical path: event → fetch → print → ack. All dedup, ack ordering, and RAW datatype enforcement lives here.

**Delivers:**
- `JobFetcher`: `GET /api/agent/jobs/{jobId}/bytes`; base64 decode; dedup via `INSERT OR IGNORE INTO printed_jobs`
- `PrintWorker`: winspool path (`WritePrinter` with `pDatatype = "RAW"`) for `printer_type = "windows_printer"`; `serialport` path for `printer_type = "serial_port"`; post-write `GetPrinter` status poll (up to 5s)
- SQLite: `status = 'printing'` on dispatch; `status = 'done'` before AckSender
- `AckSender`: POST ack only after `done` written; 409 treated as success
- Graceful handling of 404 (expired job) and 409 (already printed) from bytes endpoint

**Addresses:** C1 (RAW datatype enforced everywhere), C3 (SQLite dedup), C4 (ack only after done), M1 (two print paths), M2 (post-write status polling), m4 (INSERT OR IGNORE atomic dedup)

**Blocked by:** Noren `GET /api/agent/jobs/{jobId}/bytes` and `POST /api/agent/jobs/{jobId}/ack`; server-side ESC/POS rendering (the longest-lead Noren prerequisite)

### Phase 6: Resilience

**Rationale:** Field hardening for printer-offline and internet-outage scenarios — both daily occurrences in restaurant environments.

**Delivers:**
- `RetryScheduler`: 3× / 30s retry via `tokio::time::sleep`; SQLite `attempt` update; tray red + Windows toast on exhaustion (plain language: "Impressora sem papel — recarregue a bobina")
- `PendingPoller`: `GET /api/agent/jobs/pending` on every Pusher reconnect; dedup before enqueuing; drain in `createdAt ASC` order
- `tauri-winrt-notification` integration with correct AppUserModelId
- Agent heartbeat: `POST /api/agent/heartbeat` every 60s — enables Noren dashboard visibility and AV quarantine detection
- Tray icon state machine: green / yellow (reconnecting) / red (printer error)

**Addresses:** M2 (retry on status poll failure), C3 (pending pull uses SQLite dedup), m4 (dedup race condition), M9 (heartbeat enables quarantine detection)

**Blocked by:** Noren `GET /api/agent/jobs/pending`; `POST /api/agent/heartbeat` (for heartbeat feature)

### Phase 7: Auto-Update + Distribution Polish

**Rationale:** Silent auto-update on reboot is the only viable update model for restaurant operators who do not schedule maintenance windows.

**Delivers:**
- `UpdateChecker`: polls `GET /api/agent/version` nightly; compares semver; downloads via Velopack; schedules update for next reboot; tray balloon notification
- Velopack bootstrapper flow validated (spike first before committing)
- Fallback if Velopack immature: download to `%APPDATA%\BrevlyPrint\update\` + `RunOnce` registry key
- SHA256 verification before scheduling any update
- Submission to Microsoft Defender Intelligence portal

**Addresses:** M6 (Velopack bootstrapper avoids locked EXE), M5 (Defender portal submission)

**Blocked by:** Noren `GET /api/agent/version`; update hosting (S3/Cloudflare)

**Research flag:** Spike Velopack Rust SDK first — production reports from pure-Rust users are sparse as of mid-2025. Allot 1–2 days before committing.

### Phase Ordering Rationale

- Foundation spike before GUI: resolves the highest-risk unknown (C2 thread model) before any UI code is written
- Activation before Pusher: agentToken (issued at activation) is required for Pusher auth; cannot test Pusher without a bound serial
- Pusher before Job Pipeline: the pipeline has no input without the event stream
- Job Pipeline before Resilience: retry and pending-pull wrap the core pipeline
- Phase 3 (tray + distributable) placed before Pusher: first signed binary is built before any network-facing code ships

### Research Flags

Needs deeper research or spike during planning:
- **Phase 1:** `tao` + raw `egui` (not eframe) integration — no confirmed reference implementation; mandatory spike; highest-risk unknown in the project
- **Phase 4:** Pusher reconnect under real network conditions — zombie connection behavior needs integration testing; >5min outage simulation required
- **Phase 7:** Velopack Rust SDK maturity — sparse pure-Rust production reports; spike before committing; fallback path documented

Standard patterns (skip research-phase):
- **Phase 2:** Windows print spooler `WritePrinter` RAW — stable Win32 API; well-documented on Microsoft Learn
- **Phase 2:** DPAPI credential storage — well-documented; `windows-dpapi` wraps standard Win32 API
- **Phase 3:** Authenticode signing + `vpk` packaging — documented process; no novel integration
- **Phase 5:** `reqwest` + `rusqlite` — de facto standard patterns
- **Phase 6:** `serialport` COM port path — stable, trivial usage

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | MEDIUM-HIGH | All crates verified; egui+tao integration and Velopack Rust SDK are the weak spots requiring spikes |
| Features | HIGH | Grounded in QZ Tray, PrintNode, CloudPRNT behavior; cross-validated with ESC/POS specs; feature set well-understood |
| Architecture | HIGH | Win32/Tokio thread model confirmed by `tray-icon` docs; data flow and dedup design are clean; minor crate errors in ARCHITECTURE.md (NWG, escpos direct path) now reconciled |
| Pitfalls | HIGH | Critical pitfalls sourced from official Microsoft docs, Pusher GitHub issues, Tauri issue tracker; well-evidenced |

**Overall confidence: MEDIUM-HIGH**

### Gaps to Address

- **tao + egui integration path:** No reference implementation confirmed. Phase 1 spike is mandatory. If egui embedding into tao proves unworkable, the subprocess approach changes Phase 2 implementation significantly.

- **Velopack Rust SDK production readiness:** MEDIUM confidence only. Spike early in Phase 7 before committing. RunOnce fallback is documented but adds complexity.

- **Noren backend readiness timeline:** The entire agent integration is blocked until Noren implements the `/api/agent/` contract. Server-side ESC/POS rendering (migrating `buildTicket` et al. from `src/lib/utils/ticket.ts`) is the longest-lead item. Noren development must start in parallel with — or before — agent Phase 4.

- **ISO-8859-1 encoding preservation in Noren migration:** Node.js string handling defaults to UTF-8; explicit Buffer encoding is required. Must be validated with real printer output for Portuguese accented characters before agent Phase 5 can be considered complete.

- **SmartScreen reputation building:** Cannot be resolved before launch. Plan for 2–6 weeks of adoption friction. Prepare installation guide with "More info → Run anyway" screenshots.

- **`WritePrinter` status polling reliability:** Post-write `GetPrinter` status polling is a heuristic with uncertain timing. Some drivers may not set `PRINTER_STATUS_PAPER_OUT` reliably. USB printer status is best-effort; serial DLE EOT is more reliable but model-dependent.

---

## Sources

### Primary (HIGH confidence)

- [tray-icon docs.rs](https://docs.rs/tray-icon/latest/tray_icon/) — Win32 main-thread requirement confirmed
- [windows crate Win32::Graphics::Printing](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Graphics/Printing/) — WritePrinter, OpenPrinterW, DOC_INFO_1W
- [Microsoft Learn: Send raw data to printer](https://learn.microsoft.com/en-us/previous-versions/troubleshoot/windows/win32/win32-raw-data-to-printer) — RAW datatype requirement
- [Pusher Channels WebSocket Protocol spec](https://pusher.com/docs/channels/library_auth_reference/pusher-websockets-protocol/) — socket_id, ping/pong, auth format
- [Pusher: Authorizing users](https://pusher.com/docs/channels/server_api/authorizing-users/) — HMAC auth signature format
- [Pusher message size limit 10KB](https://docs.bird.com/pusher/channels/channels/limits/what-is-the-message-size-limit-when-publishing-an-event-in-channels) — justifies HTTP payload path
- [ESC/POS DLE EOT status](https://escpos.readthedocs.io/en/latest/reliance_status.html) — paper-end detection
- [Microsoft: SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation) — OV cert policy post March 2024
- [Velopack Getting Started Rust](https://docs.velopack.io/getting-started/rust) — Rust SDK existence confirmed
- [CryptProtectData DPAPI](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata) — Scope::User semantics
- [egui + eframe event loop conflict — emilk/egui issue #2875](https://github.com/emilk/egui/issues/2875) — confirms C2 pitfall
- [Microsoft Learn: WritePrinter](https://learn.microsoft.com/en-us/windows/win32/printdocs/writeprinter)
- [V4 print driver RAW mode 0-byte spool](https://learn.microsoft.com/en-us/troubleshoot/windows/win32/v4-print-driver-raw-mode-pcl-postscript)

### Secondary (MEDIUM confidence)

- [QZ Tray LNA / Chrome 147](https://qz.io/docs/lna) — strategic justification for the project; search-verified
- [Pusher silent disconnect >5min — pusher-websocket-java #210](https://github.com/pusher/pusher-websocket-java/issues/210) — zombie connection pattern
- [Pusher silent disconnect — pusher-websocket-swift #171](https://github.com/pusher/pusher-websocket-swift/issues/171) — corroborating issue
- [SmartScreen EV/OV equivalence March 2024 — ToDesktop blog](https://www.todesktop.com/blog/posts/windows-apps-psa-ev-certs-do-not-grant-immediate-reputation-anymore)
- [self_update locked EXE on Windows — jaemk/self_update](https://github.com/jaemk/self_update)
- [native-windows-gui "3rd and final version" — crates.io](https://crates.io/crates/native-windows-gui)
- [pusher-http-rust — explicitly Unsupported](https://github.com/WillSewell/pusher-http-rust)
- [sled maintenance mode](https://github.com/spacejam/sled)
- [Rust antivirus false positives — Rust Forum](https://users.rust-lang.org/t/why-my-windows-defender-think-my-rust-file-is-trojan/111832)
- [Tauri Trojan false positive — tauri-apps/tauri #2486](https://github.com/tauri-apps/tauri/issues/2486)
- [Star CloudPRNT polling interval (5-10s)](https://star-m.jp/products/s_print/sdk/StarCloudPRNT/manual/en/protocol-reference/http-method-reference/server-polling-post/polling-timing.html)

### Tertiary (LOW confidence, needs field validation)

- USB thermal printer virtual COM port behavior (M1) — documented via Device Manager observation patterns; clone behavior varies
- WritePrinter status polling timing reliability — heuristic; driver behavior varies by manufacturer
- ESC/POS DLE EOT availability on non-Epson clones — clone firmware implementations vary significantly

---

*Research completed: 2026-07-15*
*Ready for roadmap: yes*
