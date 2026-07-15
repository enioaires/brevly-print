# Feature Landscape

**Domain:** Unattended thermal-receipt print agent (Windows POS / restaurant)
**Researched:** 2026-07-15
**Confidence:** HIGH (grounded in QZ Tray, PrintNode, Star CloudPRNT behavior; cross-validated with ESC/POS specs)

---

## Table Stakes

Features that users (restaurant owner, cashier) expect to work out of the box. Missing any of these causes the agent to fail in the field or get uninstalled.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| **Autostart with Windows** | Restaurant PCs are rebooted by staff who never think about software — agent must survive reboot without anyone touching it. QZ Tray 2.1+ does this automatically. | Low | Registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` or Windows Task Scheduler at login. Must work for standard (non-admin) user sessions. |
| **Invisible operation** | The cashier must never interact with the agent. Any UI prompt = noise = complaints. QZ Tray's WebSocket prompt in Chrome 147 broke dozens of deployments — this is the exact problem Brevly replaces. | Low | System tray icon only. No splash screen, no popups during normal operation. |
| **System tray icon with connection status** | Non-technical staff need a glanceable signal. Industry norm: green = all good, red/yellow = something wrong. QZ Tray used a colored icon (older versions green, newer white/black). PrintNode uses a tray icon. | Low | Two states minimum: CONNECTED (green) and PROBLEM (red). See error signaling section. |
| **One-time activation UX** | Owner installs once and enters a serial/license code to bind the agent to their Noren tenant. This is the only moment the owner interacts. Must be idiot-proof (no CLI, no JSON editing). | Medium | Window with: serial input field, printer dropdown (enumerated from Windows), Save button. Validates serial against backend before saving. Blocks startup until activated. |
| **Printer selection at activation** | Owner picks from dropdown of installed Windows printers. Agent must enumerate via WinAPI (`EnumPrintersW`). No manual entry of port/driver paths. | Low | Windows spooler gives this for free. Must filter to only physical printers (exclude PDF, Fax, etc.). |
| **Near-instant print (<1s end-to-end)** | This is the product's core value. CloudPRNT HTTP polling achieves 5-10s at best. QZ Tray + WebSocket achieves ~200-500ms but requires open browser tab. Pusher push + HTTP fetch is achievable in <500ms. Any delay >2s causes staff to assume it broke and reprint manually, creating duplicates. | Medium | Depends on Pusher event latency (~100-200ms) + HTTP fetch of ESC/POS bytes (~100ms on LAN-equivalent localhost backend) + Windows WinAPI print call (~50ms). |
| **Dumb spooler — accept raw ESC/POS bytes** | Noren renders the ESC/POS bytes server-side. The agent must pass them verbatim to the Windows printer via `WritePrinter` with RAW data type. No re-encoding, no re-layout. | Low | `OpenPrinterW` + `StartDocPrinterW` with `DOCINFOW.pDatatype = "RAW"` + `WritePrinter` + `EndDocPrinterW`. Standard WinAPI pattern. |
| **Automatic retry on printer error (local)** | Paper jams, cover open, power-cycled printer — common in kitchens. Agent must retry without human intervention on the agent PC. | Medium | Up to 3 retries with 30s interval (per PROJECT.md). Retry only recoverable errors (paper-out, cover open, offline). Permanent failure after 3 attempts triggers notification. |
| **Windows toast notification on permanent failure** | After retries exhausted, staff need to know so they can reload paper or check the printer. Must reach a non-technical cashier. | Low | Windows `ToastNotification` API (WinRT) or `ShellNotifyIcon` balloon. Message must be plain language: "Impressora sem papel — recarregue e o sistema tentará novamente." |
| **Offline job queue (server-side pull on reconnect)** | Internet drops during lunch rush = lost orders = chaos. Noren already records `kitchen_printed_at` / `dispatch_printed_at`; the agent must pull unprinted jobs when it reconnects. | Medium | On Pusher reconnect event: call `GET /api/print-jobs/pending` (authenticated with serial/token). Backend returns unprinted jobs in order; agent drains queue. No local queue persistence needed (server is the source of truth). |
| **Serial-based authentication** | Agent must prove to Noren it belongs to a specific tenant before receiving print events. Serial maps to tenant. All HTTP calls carry the serial as Bearer or in headers. | Medium | Backend generates serial, stores serial→tenant mapping. Agent sends serial in Pusher auth request and in HTTP job fetch. |
| **USB + serial port printing** | Epson TM-T20X uses USB (shows as Windows print queue) or serial COM port. Must support both. QZ Tray supports both. | Medium | USB: WinAPI `WritePrinter` via spooler. Serial: `CreateFile` on `\\.\COM3` + `WriteFile` at correct baud rate (9600 or 115200 per printer config). |

---

## Differentiators

Features that set Brevly Print apart from QZ Tray and cloud agents. Not expected by day-1 users, but create stickiness and reduce support burden.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| **No browser tab required** | QZ Tray's critical failure mode: Chrome 147 (April 2026) introduced LNA (Local Network Access) restrictions that block `ws://localhost:8181` without an explicit user prompt. Brevly Print has no browser dependency — agent is always-on regardless of whether the Noren tab is open. | None (architectural) | This is the main reason Brevly Print exists. Zero extra code needed; it's a consequence of the push/HTTP architecture. |
| **Per-event-type enable/disable (sourced from Noren)** | Restaurant may want comanda de pedido but not cupom de fechamento on a kitchen printer. Feature flags controlled from Noren dashboard, synced to agent at activation or on reconnect. | Medium | Noren pushes event-type config in the Pusher auth response or as part of the activation HTTP response. Agent checks bitmask before printing. Three flags: `print_order`, `print_dispatch`, `print_closing`. |
| **Self-healing reconnect with exponential backoff** | Pusher Rust client (`pusher-rs`) supports configurable reconnect with backoff. Key insight from Pusher issues: after >5 minutes offline, auto-reconnect can fail silently. Agent must implement explicit reconnect health check (ping/pong or periodic re-subscribe). | Medium | On `ConnectionState::Disconnected`: exponential backoff (1s→2s→4s→max 60s). On reconnect: re-subscribe to channel, then immediately pull pending jobs. Tray icon transitions to yellow "reconnecting" then green on success. |
| **HTTP job fetch + delivery ack (confirmed delivery)** | Unlike QZ Tray (fire-and-forget from browser), Brevly Print fetches job bytes via authenticated HTTP and POSTs an ack to Noren after the WinAPI print call succeeds. Noren can then mark `kitchen_printed_at` server-side. This enables the server-side queue to know what's been delivered. | Medium | `GET /api/print-jobs/{jobId}/bytes` → print → `POST /api/print-jobs/{jobId}/ack` with `{status: "printed" | "failed", timestamp}`. Noren uses ack to clear the job from pending queue. |
| **Agent heartbeat to backend** | Backend knows agent is alive without waiting for a print event. Enables Noren dashboard to show "Impressora: Online / Offline" per tenant without relying on Pusher presence channels. | Low | HTTP `POST /api/agent/heartbeat` every 60s with `{serial, version, printer_status}`. Backend marks agent as offline if no heartbeat for 5 minutes. |
| **Printer status reporting in heartbeat** | Heartbeat carries printer status (paper OK / paper near-end / cover open / offline) detected via ESC/POS `DLE EOT n` status command. Noren can surface this to the owner dashboard proactively ("Papel acabando na impressora da caixa"). | High | Requires sending ESC/POS status enquiry byte (`0x10 0x04 0x01`) to printer and parsing 1-byte response. Only possible if agent directly controls the port (serial mode). Windows spooler (USB mode) does not reliably expose paper-out status — this may be serial-only. Flag as LOW confidence for USB path. |
| **Silent auto-update on next restart** | No IT team at a restaurant. Bugs must be fixed without the owner doing anything. PrintNode notifies users via Twitter; that is inadequate. Agent must self-update silently. | High | Agent polls `GET /api/agent/version` on startup. If newer version available, downloads installer to temp dir, registers a "run once" registry key to launch installer silently (`/VERYSILENT /SUPPRESSMSGBOXES`) on next login, notifies tray with balloon: "Atualização disponível — será instalada no próximo reinício." |

---

## Anti-Features

Things to explicitly NOT build in v1. Each has a reason. These should not re-enter scope without a strong forcing function.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| **Elaborate configuration UI** | Agent is invisible. Any config UI creates support surface (owners tinkering, breaking things). The activation screen is the only necessary UI. | All config comes from Noren dashboard (event type flags, etc.) and is pushed at connect time. |
| **Print history UI in the agent** | Noren is the source of truth. Local history duplicates data, adds SQLite dependency, and invites owners to second-guess Noren's records. | Let owners view print history in the Noren dashboard, where jobs are already stored with timestamps. |
| **Mac support** | All Noren customers run Windows POS terminals. Mac adds separate code paths for CUPS, different autostart mechanism, different UI framework. Zero ROI v1. | Revisit if customer base changes. |
| **Network printers** | Network discovery (mDNS/Bonjour, SNMP) adds 2-3x complexity. Restaurant printers are USB. | USB + serial only. Owner connects printer physically. |
| **ESC/POS generation in the agent** | If agent generates ESC/POS, templates must be maintained in both Noren (for current behavior) and in Rust (for agent). Layout drift, QR code differences, character encoding bugs. | Noren renders bytes server-side. Agent is a dumb pipe. ESC/POS knowledge stays in one place. |
| **Multiple printers per agent** | 1 agent = 1 printer keeps the activation flow trivial and the state machine simple. Multi-printer requires printer routing logic, per-printer event subscriptions, and a config UI. | If a restaurant needs kitchen + counter printing, they install two agents on two machines (or one machine with two agents). Revisit as v2. |
| **Local job persistence (SQLite/file queue)** | If internet is down, the server-side queue is the source of truth. A local queue introduces sync conflicts, stale jobs, and corruption scenarios. | Server-side pull on reconnect covers the offline case cleanly. |
| **Cash drawer kick** | Noren does not currently use cash drawer control. Adding `ESC p` command requires mapping printer model → drawer port, which varies. | Out of scope until Noren explicitly requires it. |
| **Generating QR codes in the agent** | The dispatch comanda has a QR code (`GS(k` command). Noren already generates the correct bytes. Duplicating QR logic in Rust risks version drift. | Noren renders the full ESC/POS block including QR. Agent passes it verbatim. |

---

## Feature Dependencies

```
Serial activation → All other features
  (nothing works without a validated serial binding the agent to a tenant)

Serial activation → Pusher channel subscription
  (Pusher auth uses serial to get tenant channel key from backend)

Pusher subscription → Receiving print events
  (events come over private-tenant-{tenantId}-kitchen channel)

Receiving print event → HTTP job fetch
  (event is a wakeup; bytes come via HTTP)

HTTP job fetch → WinAPI print call
  (bytes are passed to WritePrinter)

WinAPI print call → Delivery ack
  (ack only sent after print call returns success)

WinAPI print call → Retry logic
  (retry only on print failure)

Retry logic (exhausted) → Windows toast notification

Pusher reconnect → Server-side queue pull
  (pull pending jobs from /api/print-jobs/pending after reconnecting)

Autostart → Headless-always-on invariant
  (everything else depends on agent being running)

Agent heartbeat → Remote status visibility in Noren
  (heartbeat is prerequisite for dashboard "printer online" indicator)

Printer status reporting → Heartbeat payload
  (status is carried in heartbeat; depends on printer status detection working)
```

---

## MVP Recommendation

Build in this order (each layer depends on the previous):

1. **Activation screen + serial validation** — unblocks everything; cannot test anything else without a bound serial
2. **Autostart + headless tray icon** — establishes the always-on invariant
3. **Pusher connection + private channel auth** — core event pipeline
4. **HTTP job fetch + WinAPI raw print** — core print path (USB mode first, serial second)
5. **Connection status indicator** (green/red tray icon) — first operator signal
6. **Retry logic + Windows toast on failure** — field reliability
7. **Server-side queue pull on reconnect** — offline resilience

Defer (post-MVP, v1.1):
- Per-event-type enable/disable (needs Noren dashboard UI to configure; build both sides together)
- Silent auto-update (needs update server endpoint; coordinate with Noren backend milestone)
- Heartbeat + remote status (needs Noren dashboard "printer status" widget; coordinate)
- Printer status reporting via ESC/POS DLE (needs Epson TM-T20X testing; HIGH complexity, uncertain USB behavior)
- Serial COM port support (USB covers most cases; serial is secondary)

---

## Real-World Operational Expectations

### How agents signal failure to non-technical staff

From researching QZ Tray, PrintNode, and POS printer failure patterns:

- **Tray icon is primary**: staff learn to glance at the taskbar. Red = call the owner. Green = all good. This is universal across QZ Tray, PrintNode, and PaperCut.
- **Windows toast notification is secondary**: appears in bottom-right corner for ~5 seconds. Must use plain language, not technical jargon. "Impressora sem papel — recarregue a bobina" beats "PrinterError: PAPER_END state on COM3."
- **Audio cues are out of scope** but note: commercial kitchen printers (Star, Epson) have physical buzzer/LED for paper-end. The agent should not fight with or suppress these — they are complementary.
- **What staff actually do**: reload paper, power-cycle printer, open Noren and click "reimprimir" — in that order. The agent's retry logic should handle the first two without requiring the third.

### How to distinguish printer-out-of-paper vs printer-offline vs internet-down

These require different responses from staff and different recovery paths:

| Error Type | Detection Method | Agent Behavior | Staff Signal |
|------------|-----------------|---------------|-------------|
| **Printer out of paper** | ESC/POS `DLE EOT 1` status byte (bit 5 = paper end). Serial mode only — Windows USB spooler does not expose this reliably via `WritePrinter` error codes. | Retry with 30s interval (paper reload expected). Tray icon red. Toast: "Impressora sem papel." | "Recarregue a bobina da impressora." |
| **Printer offline** (power off / USB disconnected) | `WritePrinter` returns error / `OpenPrinterW` fails / no status response on serial. | Retry with 30s interval. Tray icon red. Toast: "Impressora desconectada." | "Verifique o cabo USB e ligue a impressora." |
| **Print job failure** (cover open, head overheat) | `WritePrinter` error or ESC/POS error status. | Same retry logic. Toast with generic "Impressora com erro — verifique a impressora." | Physical LED on printer indicates specific error. |
| **Internet down** (Pusher disconnected) | Pusher connection state changes to `Disconnected`. | Self-healing reconnect with exponential backoff. Tray icon yellow "reconnecting." On reconnect: pull pending jobs. | Tray transitions back to green automatically — no staff action needed if job queue works. |
| **Backend down** (HTTP 5xx on job fetch) | HTTP client returns non-2xx on `/api/print-jobs/{id}/bytes`. | Retry HTTP fetch 3× (1s apart) before considering job failed. If backend recovers, pending queue pull on next successful connect handles it. | Tray yellow during backend outage; auto-resolves. |

### What owners expect from install/activation UX

Based on research into PrintNode, ThinPrint, and similar unattended agent deployments:

- **Download from a URL**: owner gets a link in the Noren onboarding email. Clicks link, downloads `.exe`, double-clicks. Standard Windows installer flow. No command line.
- **One activation screen**: serial number input (auto-formatted, uppercase, copy-paste friendly) + printer dropdown + "Ativar" button. No multi-step wizard.
- **Instant validation feedback**: "Serial válido — impressora Epson TM-T20X selecionada" or "Serial inválido — verifique o e-mail de boas-vindas." Not a loading spinner that times out.
- **Never see it again**: after activation, agent disappears into the tray. Owner should not need to touch it again. Any required interaction (paper reload, reboot) must be prompted via the tray/toast, not via the app UI.
- **Silent update**: owners at restaurants do not schedule maintenance windows. Auto-update on reboot is the only viable model. PrintNode's approach of emailing users to update manually is explicitly inadequate for this user profile.

---

## Sources

- QZ Tray LNA / Chrome 147 breaking change: https://qz.io/docs/lna (MEDIUM confidence — search-verified, WebFetch denied)
- QZ Tray autostart behavior: https://github.com/qzind/tray/issues/4 (MEDIUM confidence)
- QZ Tray WebSocket architecture: https://github.com/qzind/tray/wiki/Architecture (MEDIUM confidence)
- PrintNode silent install flags: https://www.printnode.com/en/docs/installation (MEDIUM confidence)
- PrintNode features for POS: https://www.printnode.com/en/use-cases (MEDIUM confidence)
- Star CloudPRNT polling interval (5s default, 5-10s delivery): https://star-m.jp/products/s_print/sdk/StarCloudPRNT/manual/en/protocol-reference/http-method-reference/server-polling-post/polling-timing.html (HIGH confidence)
- ESC/POS status detection (paper-end bit 5, offline = no response): https://escpos.readthedocs.io/en/latest/reliance_status.html (HIGH confidence)
- Windows USB spooler ESC/POS limitation (generic driver lacks paper-out): search result from tcang.net/pos-machine-not-printing (MEDIUM confidence)
- Pusher reconnect edge cases (>5 min offline): https://github.com/pusher/pusher-websocket-java/issues/210 (MEDIUM confidence — behavior may vary by SDK; Rust client should be tested)
- pusher-rs Rust library (supports private channels, configurable reconnect): https://lib.rs/crates/pusher-rs (MEDIUM confidence — lib.rs listing, not official Pusher docs)
- WinAPI raw printing via Rust: https://users.rust-lang.org/t/raw-printer-in-rust/58139 + https://github.com/talesluna/rust-printers (MEDIUM confidence)
- Windows toast notification for print errors: Microsoft Learn (working-with-print-notifications) (HIGH confidence — official)
- Restaurant POS printer failure UX: https://alexandriacomputers.com/troubleshooting-pos-receipt-printers-common-errors-solutions/ (MEDIUM confidence)
