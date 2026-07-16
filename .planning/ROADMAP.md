# Roadmap: Brevly Print

**Project:** Brevly Print — Native Windows Thermal Print Agent
**Milestone:** v1 MVP
**Created:** 2026-07-15
**Granularity:** standard (7 phases)

---

## Phases

- [x] **Phase 1: Foundation + Thread Model Spike** - Prove the `winit 0.30` + raw `egui` (egui-winit + egui-wgpu) event-loop integration; initialize SQLite schema (rusqlite_migration v1) and DPAPI credential infra
- [x] **Phase 2: Activation** - One-time activation window with serial validation, printer selection, DPAPI credential storage, and autostart registration (completed 2026-07-16)
- [x] **Phase 3: Tray + Runtime + First Distributable** - Always-on tray agent with Win32/Tokio bridge, startup recovery, and first signed installer (completed 2026-07-16)
- [x] **Phase 4: Pusher Event Stream** - Hand-rolled Pusher WebSocket client with HMAC channel auth, ping/pong health check, and reconnect logic (completed 2026-07-16)
- [x] **Phase 5: Job Pipeline** - End-to-end print path: event → fetch bytes → WritePrinter/serial → SQLite dedup → ack (completed 2026-07-16)
- [x] **Phase 6: Resilience** - Printer-failure retry, Windows toast notifications, offline job pull, and boot-crash recovery (completed 2026-07-16)
- [ ] **Phase 7: Auto-Update + Distribution Polish** - SHA256-verified silent auto-update via Velopack on next reboot

---

## Phase Details

### Phase 1: Foundation + Thread Model Spike
**Goal**: Prove the `winit 0.30` + raw `egui` (`egui-winit` + `egui-wgpu`) event-loop integration and initialize all persistence infrastructure on a **cross-platform-buildable** base (portable core builds+tests on Linux AND Windows; product v1 stays Windows-only) so every subsequent phase builds on a validated base
**Mode:** mvp
**Depends on**: Nothing
**Requirements**: (none — pure technical spike; unblocks all v1 requirements)
**Success Criteria** (what must be TRUE):
  1. A minimal `winit 0.30` `ApplicationHandler` event loop drives a raw `egui`-rendered window (`egui-winit` + `egui-wgpu`) without a separate Win32 message loop — proven on **Linux** (Vulkan/GL) and confirmed on **Windows** (DX12) (or the subprocess alternative is proven and documented as the Phase 2 approach). *[Revised 2026-07-15: `winit` replaces `tao`; and window is now cross-platform per the "works on Linux too" requirement — see Phase 1 CONTEXT/RESEARCH.]*
  2. SQLite `state.db` initializes with tables `config`, `printed_jobs`, `retry_queue` on first run at the per-platform app dir (`%APPDATA%\BrevlyPrint\` on Windows, `~/.local/share/BrevlyPrint/` on Linux via `dirs`); verified by `cargo test` on **both** platforms
  3. Credentials round-trip through a `CredentialStore` trait; missing/corrupt → typed `CredentialError` (never panics) on both impls. Windows impl uses DPAPI `Scope::User` (real round-trip proven on Windows); a Linux dev impl exists so the trait/error contract tests pass on Linux
  4. Cargo compiles the **portable core on `x86_64-unknown-linux-gnu`** AND the **full v1 dep set on `x86_64-pc-windows-msvc`** (Windows-only crates — `windows`, `windows-dpapi`, `tray-icon`, `printers`, `auto-launch`, `velopack`, `tauri-winrt-notification` — under `[target.'cfg(windows)'.dependencies]`) — verified versions per Phase 1 RESEARCH.md §Standard Stack
**Plans**: 3 plans (replanned 2026-07-15 for cross-platform)
- [x] 01-01-PLAN.md — Cross-platform Cargo scaffold (target-gated deps), app-dir init, CredentialStore trait + cfg impls, Wave-0 test scaffolds, ubuntu+windows CI matrix
- [x] 01-02-PLAN.md — ConfigStore (rusqlite_migration v1, 3 tables, get/set) + both credential impls; Linux-provable contract tests + Windows DPAPI tests
- [x] 01-03-PLAN.md — Walking skeleton: winit 0.30 ApplicationHandler + raw egui window wiring the stores end-to-end (SC-1), human-verify checkpoint APPROVED (Linux + Windows)

### Phase 2: Activation
**Goal**: Users (restaurant owners) can install the agent, enter a serial number, select a printer, test-print, and save — resulting in a bound, autostarting agent ready for operation
**Mode:** mvp
**Depends on**: Phase 1
**Depends on (Noren)**: `POST /api/agent/activate` endpoint live; `agent_serials` table in DB schema
**Requirements**: ACT-01, ACT-02, ACT-03, ACT-04, ACT-05, ACT-06, ACT-07, ACT-08
**Success Criteria** (what must be TRUE):
  1. The Windows installer (`.exe`) can be downloaded and installs the agent as a normal program (appears in Add/Remove Programs)
  2. On first launch, an activation window opens showing a serial number input field and a printer dropdown listing both installed Windows printers and available COM ports
  3. Entering a valid serial and clicking Activate shows immediate visual feedback; an invalid serial shows an error message without closing the window
  4. The test-print button sends raw `ESC @` + `GS V` cut bytes to the selected printer before saving — bytes reach the hardware
  5. After saving, the agent exits the activation window, registers itself to start with Windows (HKCU Run), and the activation window does not appear again on subsequent launches
  6. After a Windows reinstall (DPAPI key loss), the agent detects the unreadable credential and re-enters the activation flow automatically
**Plans**: 3 plans (2 waves)
- [x] 02-01-PLAN.md — Contracts + Linux-provable seams: noren_client (serial validation), Printer trait + Linux stub, machine_id, winreg dep, Wave-0 tests (ACT-03/04/05)
- [x] 02-02-PLAN.md — Windows hardware impls: WritePrinter RAW spooler (C1), serialport write, combined printer enumeration (ACT-04/05)
- [x] 02-03-PLAN.md — Activation window slice: startup credential branch, egui UI-SPEC form + async validation, test-print, save→DPAPI+SQLite+HKCU autostart→exit (ACT-01 PARTIAL, ACT-02/06/07/08)
**UI hint**: yes

### Phase 3: Tray + Runtime + First Distributable
**Goal**: The agent runs invisibly after Windows login, displays a tray icon reflecting its connection state, survives reboots without intervention, and ships as a signed installer
**Mode:** mvp
**Depends on**: Phase 2
**Requirements**: RUN-01, RUN-02, RUN-03, DIST-01
**Success Criteria** (what must be TRUE):
  1. After the machine reboots, the agent starts automatically and a tray icon (green or red) appears in the system tray — no user action required
  2. The tray icon is green when the agent is healthy, yellow while reconnecting, and red when a connection or printer problem is detected
  3. The agent has no open windows during normal operation — no taskbar entry, no pop-up; only the tray icon
  4. The distributed `.exe` installer is Authenticode-signed (OV certificate); Windows does not block or warn "Unknown publisher" when running the installer
**Plans**: TBD

### Phase 4: Pusher Event Stream
**Goal**: The agent subscribes to its tenant's private Pusher channel, receives print-event notifications reliably, and recovers from network interruptions without missing events
**Mode:** mvp
**Depends on**: Phase 3
**Depends on (Noren)**: `POST /api/agent/pusher/auth` endpoint live (HMAC + hard 403 on wrong channel); Pusher event emission on order/dispatch/close transitions sending `{jobId, type}`
**Requirements**: EVT-01, EVT-02, EVT-03
**Success Criteria** (what must be TRUE):
  1. After activation, the agent connects to its private Pusher channel (`private-tenant-{tenantId}-print`) and the tray icon turns green
  2. When the Noren backend emits a print event, the agent receives `{jobId, type}` within 500 ms and enqueues it for processing
  3. When the internet connection drops for more than 5 minutes and is restored, the agent detects the zombie connection (via ping/pong timeout), reconnects with exponential backoff, re-authenticates with the new `socket_id`, re-subscribes to the channel, and the tray transitions yellow then green — no events missed
  4. Channel auth is re-requested from `/api/agent/pusher/auth` on every reconnect using the fresh `socket_id`; cached auth strings are never reused
**Plans**: 2 plans (2 waves)
- [x] 04-01-PLAN.md — Tested primitives: futures-util+rand deps, D-01 config persistence fix, pusher protocol types+parsers (double-decode, socket_id), backoff, pusher_auth() + HTTP-contract tests (EVT-01/02/03 seams)
- [x] 04-02-PLAN.md — Vertical slice: run_pusher_loop reconnect state machine (connect→fresh auth→subscribe→ping/pong zombie→backoff), INSERT OR IGNORE dedup fence, debug fake-event shim, main.rs Runtime-mode wiring (EVT-01/02/03 runtime)

### Phase 5: Job Pipeline
**Goal**: Every print event results in the correct ESC/POS bytes being written to the thermal printer within 1 second, with delivery confirmed back to Noren and duplicate prints prevented
**Mode:** mvp
**Depends on**: Phase 4
**Depends on (Noren)**: `GET /api/agent/jobs/{jobId}/bytes` live (returns base64 ESC/POS); `POST /api/agent/jobs/{jobId}/ack` live (idempotent, 409 on repeat); server-side ESC/POS rendering complete (migrated from `ticket.ts`, preserving ISO-8859-1 encoding for Portuguese accented characters)
**Requirements**: PRT-01, PRT-02, PRT-03, PRT-04, PRT-05, PRT-06, PRT-07, PRT-08, PRT-09
**Success Criteria** (what must be TRUE):
  1. A comanda de pedido (order ticket) prints on the thermal printer in under 1 second from the moment the Pusher event arrives
  2. A comanda do entregador (dispatch ticket with QR code) and a cupom de fechamento (closing receipt) each print correctly when their respective events arrive — QR code scans as the dispatch token
  3. The same job delivered twice (reconnect, redelivery, or crash restart) prints exactly once — SQLite `printed_jobs` dedup prevents the second print
  4. The ack is sent to Noren only after `status = 'done'` is written to SQLite; a crash between print and ack causes the job to be re-fetched on reconnect and deduplicated, never silently lost
  5. A job type that has been disabled in the per-tenant configuration is received but not printed; no error is raised
  6. Printing works via both paths: a USB thermal printer enumerated as a Windows printer (WritePrinter RAW) and a printer on a COM port (serialport)
**Plans**: 2 plans (2 waves)
- [x] 05-01-PLAN.md — HTTP primitives: base64 dep, fetch_job_bytes() + ack_job() in noren_client.rs, Wave-0 contract tests + print_worker skeleton (PRT-01, PRT-08)
- [x] 05-02-PLAN.md — Print worker vertical slice: run_print_worker() fetch→print→UPDATE→ack pipeline, enabled_types filter, main.rs spawn wiring (PRT-02/03/04/05/06/07/09)

### Phase 6: Resilience
**Goal**: The agent handles printer failures and internet outages gracefully — retrying locally, alerting the operator in plain language, and pulling any missed jobs on reconnect — so no ticket is permanently lost
**Mode:** mvp
**Depends on**: Phase 5
**Depends on (Noren)**: `GET /api/agent/jobs/pending` live (unacked jobs sorted by `createdAt ASC`, max 100)
**Requirements**: RES-01, RES-02, RES-03, RES-04
**Success Criteria** (what must be TRUE):
  1. When the printer fails (paper out, offline), the agent retries the print job 3 times with a 30-second interval before giving up
  2. After 3 failed retries, a Windows toast notification appears with a plain-language message (e.g., "Impressora sem papel — recarregue a bobina") and the tray icon turns red
  3. When the internet connection is restored after an outage, the agent pulls all unacked jobs from `/api/agent/jobs/pending` and prints them in chronological order — no ticket is lost during an internet outage
  4. On boot after a crash, any jobs left in `status = 'printing'` in SQLite are reprocessed; SQLite dedup prevents double-printing
**Plans**: 4 plans (4 waves)
- [x] 06-01-PLAN.md — Wave-0 test scaffolds: retry_task_test + pending_jobs_test RED stubs, config_store_test user_version 1→2 (RES-01/02/03/04)
- [x] 06-02-PLAN.md — Migration v2 ('printing' status) + print_worker status='printing' fence + retry_queue INSERT on failure (RES-01/04)
- [x] 06-03-PLAN.md — retry_task.rs (crash recovery + 5s poll + 3×/30s retry + toast/red-tray exhaustion) + health strings + main.rs spawn (RES-01/02/04)
- [x] 06-04-PLAN.md — fetch_pending_jobs() + pusher/client.rs pending-pull on reconnect with CR-02 validation + dedup (RES-03)

### Phase 7: Auto-Update + Distribution Polish
**Goal**: The agent silently downloads and applies updates on the next reboot without any action from the restaurant owner, with integrity verified before applying
**Mode:** mvp
**Depends on**: Phase 3
**Depends on (Noren)**: `GET /api/agent/version` live (returns `{version, downloadUrl, sha256}`); update binary hosted at `downloadUrl` (S3 or Cloudflare)
**Requirements**: DIST-02, DIST-03
**Success Criteria** (what must be TRUE):
  1. When a new version is available, the agent downloads it in the background without interrupting printing; the tray shows a brief notification that an update is ready
  2. The new binary's SHA256 hash is verified against the value from `/api/agent/version` before the update is scheduled — a mismatch aborts the update without touching the running agent
  3. On the next Windows reboot after a pending update, the new version is running without any manual action from the owner
**Plans**: TBD

---

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation + Thread Model Spike | 3/3 | Complete | 2026-07-15 |
| 2. Activation | 3/3 | Complete   | 2026-07-16 |
| 3. Tray + Runtime + First Distributable | 3/3 | Complete   | 2026-07-16 |
| 4. Pusher Event Stream | 2/2 | Complete   | 2026-07-16 |
| 5. Job Pipeline | 2/2 | Complete   | 2026-07-16 |
| 6. Resilience | 4/4 | Complete   | 2026-07-16 |
| 7. Auto-Update + Distribution Polish | 0/0 | Not started | - |

---

## Coverage

| Requirement | Phase | Notes |
|-------------|-------|-------|
| ACT-01 | 2 | Installer |
| ACT-02 | 2 | First-run activation window |
| ACT-03 | 2 | Serial validation via Noren |
| ACT-04 | 2 | Printer + COM port dropdown |
| ACT-05 | 2 | Test-print button |
| ACT-06 | 2 | DPAPI + SQLite config persist |
| ACT-07 | 2 | Re-activation on DPAPI failure |
| ACT-08 | 2 | Autostart registration |
| RUN-01 | 3 | Invisible operation |
| RUN-02 | 3 | Tray icon states |
| RUN-03 | 3 | Autostart + reconnect on reboot |
| EVT-01 | 4 | Pusher channel subscription |
| EVT-02 | 4 | Re-auth on reconnect |
| EVT-03 | 4 | Ping/pong zombie detection |
| PRT-01 | 5 | Fetch ESC/POS bytes |
| PRT-02 | 5 | Print comanda de pedido |
| PRT-03 | 5 | Print comanda do entregador (QR) |
| PRT-04 | 5 | Print cupom de fechamento |
| PRT-05 | 5 | WritePrinter + serial paths |
| PRT-06 | 5 | < 1 second latency |
| PRT-07 | 5 | SQLite dedup |
| PRT-08 | 5 | Ack after done |
| PRT-09 | 5 | Per-type enable/disable flags |
| RES-01 | 6 | 3× retry / 30s |
| RES-02 | 6 | Toast + red tray on exhaustion |
| RES-03 | 6 | Pending pull on reconnect |
| RES-04 | 6 | Boot crash recovery |
| DIST-01 | 3 | Authenticode signing |
| DIST-02 | 7 | Auto-update |
| DIST-03 | 7 | SHA256 integrity check |

**v1 coverage: 25/25 requirements mapped. No orphans.**

---

*Roadmap created: 2026-07-15*
*Last updated: 2026-07-16 after Phase 6 planning (4 plans, 4 waves)*
