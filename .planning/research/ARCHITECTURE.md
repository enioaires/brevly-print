# Architecture Patterns

**Project:** Brevly Print — Rust native Windows print agent
**Domain:** Background agent integrating Pusher WS + HTTP REST with thermal printer spooler
**Researched:** 2026-07-15

---

## Recommended Architecture

### High-Level Overview

The agent is split into two thread domains that never block each other:

```
┌────────────────────────────────────────────────────────────────────────┐
│  OS Main Thread (Win32 message loop)                                   │
│                                                                        │
│  ┌──────────────┐    ┌──────────────────┐    ┌─────────────────────┐  │
│  │  TrayIcon    │    │  SetupWindow     │    │  Win32 Message Loop │  │
│  │  (tray-icon) │    │  (nwg activation │    │  (GetMessage /      │  │
│  │  green/red   │    │   dialog)        │    │   DispatchMessage)  │  │
│  └──────┬───────┘    └────────┬─────────┘    └──────────┬──────────┘  │
│         │                     │                          │             │
│         └──── UiCommand (tokio::sync::mpsc) ────────────┘             │
└─────────────────────────┬──────────────────────────────────────────────┘
                          │  mpsc::Sender<UiCommand>
                          ▼
┌────────────────────────────────────────────────────────────────────────┐
│  Tokio Runtime Thread Pool (multi-threaded)                            │
│                                                                        │
│  ┌─────────────────┐    ┌──────────────────┐    ┌──────────────────┐  │
│  │  EventListener  │    │  JobFetcher      │    │  AckSender       │  │
│  │  (Pusher WS     │───▶│  (authenticated  │───▶│  (POST /jobs/:id │  │
│  │   private chan) │    │   GET bytes)     │    │   /ack)          │  │
│  └────────┬────────┘    └────────┬─────────┘    └──────────────────┘  │
│           │                      │                                     │
│           ▼                      ▼                                     │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │  JobQueue  (tokio::sync::mpsc channel, in-memory + SQLite dedup)│  │
│  └─────────────────────────────┬───────────────────────────────────┘  │
│                                │                                       │
│           ┌────────────────────┘                                       │
│           ▼                                                            │
│  ┌─────────────────┐    ┌──────────────────┐    ┌──────────────────┐  │
│  │  PrintWorker    │    │  RetryScheduler  │    │  UpdateChecker   │  │
│  │  (USB/serial    │◄───│  (3× / 30s       │    │  (self_update    │  │
│  │   raw bytes)    │    │   backoff)       │    │   crate, nightly)│  │
│  └─────────────────┘    └──────────────────┘    └──────────────────┘  │
│                                                                        │
│  ┌─────────────────┐    ┌──────────────────┐                          │
│  │  ConfigStore    │    │  CredentialStore │                          │
│  │  (SQLite /      │    │  (DPAPI-encrypted│                          │
│  │   printed_ids)  │    │   in AppData)    │                          │
│  └─────────────────┘    └──────────────────┘                          │
└────────────────────────────────────────────────────────────────────────┘
```

---

## Component Boundaries

### Thread 1 — Win32 Main Thread

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| **Win32MessageLoop** | Runs `GetMessage` / `DispatchMessage`; owns all Win32 handles | TrayIcon, SetupWindow |
| **TrayIcon** (`tray-icon` crate) | Renders green/red status icon in system tray; shows context menu | Receives `TrayUpdate` from tokio runtime via `std::sync::mpsc` (sync sender) |
| **SetupWindow** (`native-windows-gui`) | First-run modal: serial input + printer selector + Save. No webview. | Sends `ActivationRequest` over `tokio::sync::oneshot` to runtime |

**Constraint (confirmed by tray-icon docs):** On Windows, tray-icon requires a Win32 event loop on the thread where it is created. The main thread owns this loop. All other work lives in the tokio pool.

**Bridge pattern:** Use `std::sync::mpsc::SyncSender<TrayUpdate>` (sync, non-blocking `try_send`) from the async runtime to update tray color without blocking tokio. In the other direction, use `tokio::sync::mpsc::Sender<AgentCommand>` to send activation results from GUI to the runtime.

### Thread Pool — Tokio Runtime

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| **EventListener** | Maintains Pusher WebSocket; subscribes `private-tenant-{tenantId}-print`; receives `{jobId, type}` events | Sends `PrintEvent` to `JobQueue` channel |
| **PendingPoller** | On reconnect, calls `GET /jobs/pending` to pull unacked jobs | Sends `PrintEvent` entries to `JobQueue` channel |
| **JobFetcher** | Receives deduped `PrintEvent`; calls `GET /jobs/{jobId}/bytes` with Bearer token; returns raw `Vec<u8>` | Feeds `PrintJob` to `PrintWorker` |
| **PrintWorker** | Writes raw bytes to USB (`CreateFile`/`WriteFile` via `escpos` crate, usbprint.sys) or serial (`serialport` crate) | Reports `PrintResult` to `RetryScheduler`; on success sends to `AckSender` |
| **RetryScheduler** | On printer failure: re-enqueues job up to 3×, with 30s delay between attempts using `tokio::time::sleep` | Manages `PrintJob` with attempt counter; on final failure notifies `TrayIcon` via sync channel |
| **AckSender** | POST `/jobs/{jobId}/ack` after confirmed print | Async HTTP (`reqwest`) with Bearer token |
| **UpdateChecker** | Polls Noren `/agent/version` nightly; downloads and replaces binary using `self_update` crate; schedules restart at next Windows login | Independent scheduled task |
| **ConfigStore** | Reads/writes SQLite (`rusqlite`); stores: `printed_job_ids` (dedup set), `config` (printer name, per-event enable flags), `retry_queue` (jobs pending retry), `last_pull_cursor` | All async components query this |
| **CredentialStore** | Encrypts/decrypts agent token with Windows DPAPI (`windows-dpapi` crate, `Scope::User`); persists ciphertext to `%APPDATA%\BrevlyPrint\credential.bin` | Queried by EventListener and JobFetcher at startup |

---

## Data Flow

### End-to-End Job Lifecycle

```
Noren server emits Pusher event
  │
  ▼
EventListener receives: { event: "print-job", data: { jobId: "uuid", type: "order" } }
  │
  ▼
Dedup check: SELECT 1 FROM printed_jobs WHERE job_id = ? AND status IN ('printing','done')
  │  already known → discard silently
  │  unknown → INSERT INTO printed_jobs (job_id, status='pending', received_at=now())
  │
  ▼
JobQueue channel (tokio::sync::mpsc) receives PrintEvent { jobId, type }
  │
  ▼
JobFetcher: GET https://noren.app/api/agent/jobs/{jobId}/bytes
  Headers: Authorization: Bearer {agentToken}
  Response: 200 { bytes: base64 } | 404 (already deleted/expired) | 409 (already printed)
  │  404 / 409 → mark as done, no-op
  │  200 → Vec<u8> raw ESC/POS
  │
  ▼
PrintWorker: opens device handle (USB or serial), WriteFile(bytes)
  │  success → UPDATE printed_jobs SET status='done', printed_at=now()
  │           → AckSender: POST /jobs/{jobId}/ack
  │  failure → RetryScheduler
  │
  ▼
RetryScheduler (on failure):
  attempt < 3 → UPDATE printed_jobs SET attempt=attempt+1; sleep 30s; re-send to PrintWorker
  attempt = 3 → UPDATE printed_jobs SET status='failed'
              → notify TrayIcon (red icon)
              → Windows toast notification: "Impressão falhou: [type] #[jobId short]"
  │
  ▼
AckSender: POST https://noren.app/api/agent/jobs/{jobId}/ack
  Headers: Authorization: Bearer {agentToken}
  Body: { printedAt: ISO8601 }
  → Noren marks job as printed, removes from pending queue
```

### Idempotency and Dedup

Idempotency lives in the **agent's SQLite `printed_jobs` table**, not in memory.

**Rule:** Before enqueuing any job (whether from Pusher or from pending-pull), the agent checks `printed_jobs` by `job_id`. Status transitions are one-way: `pending → printing → done | failed`. A job that has reached `done` is never re-enqueued regardless of how many times Pusher delivers the event.

This handles:
- Pusher at-least-once delivery (duplicate events on reconnect)
- Pending-pull returning a job that was already received via Pusher
- Agent crash after print but before ack (status `printing` at restart → re-fetch and re-print is safe because ESC/POS bytes are idempotent to the thermal printer; but the ack POST will be retried and Noren must be idempotent on ack too — see API contract)

**Stale `printing` rows on startup:** On startup, any row with `status = 'printing'` indicates a crash mid-print. These should be re-enqueued at startup, fetching bytes again.

---

## Resilience Design

### Failure Domain A — Printer Offline / Out of Paper

```
PrintWorker fails to write bytes
  │
  ▼
RetryScheduler:
  attempt 1 → wait 30s → retry
  attempt 2 → wait 30s → retry
  attempt 3 → mark failed → red tray + Windows notification
               job stays in SQLite with status='failed'
               (operator can clear via tray context menu "Tentar novamente" in future)
```

The Pusher WebSocket and internet remain connected throughout. Ack is only sent after confirmed print; Noren keeps the job in its server queue.

### Failure Domain B — Internet / Pusher Down

```
Pusher WebSocket disconnects
  │
  ▼
EventListener detects disconnect:
  - pusher_rs crate fires reconnection with exponential backoff (built-in)
  - TrayIcon → yellow (reconnecting) via sync channel
  │
  ▼
On successful reconnect:
  1. Re-authenticate Pusher private channel (POST /api/agent/pusher/auth with socketId)
  2. Trigger PendingPoller: GET /api/agent/jobs/pending
     Response: [ { jobId, type }, ... ] sorted by createdAt ASC (oldest first)
  3. For each pending job → run through dedup check → enqueue
  │
  ▼
TrayIcon → green
```

**Key invariant:** Noren's server-side job queue (table `agent_print_jobs`) retains all unacked jobs indefinitely (or until TTL e.g. 24h). A job is removed from pending only when the agent POSTs `/ack`. This means any offline window, however long, results in a pull batch on reconnect.

**No double-print guarantee on reconnect:** The `printed_jobs` dedup table handles this. If a job arrived via Pusher before the disconnect and is already `done`, the pending-pull result for that same `jobId` is silently discarded.

---

## Activation / Auth Flow

```
First run: SetupWindow renders
  │
  ├─ User enters serial number
  ├─ User selects printer from dropdown (enumerated via SetupDiEnumDeviceInfo / serialport::available_ports)
  │
  ▼
POST https://noren.app/api/agent/activate
  Body: { serial: "XXXX-YYYY-ZZZZ" }
  Response 200: { agentToken: "opaque-long-lived-jwt", tenantId, enabledTypes: ["order","dispatch","close"] }
  Response 400: { error: "invalid_serial" }
  Response 409: { error: "already_activated" }  (serial bound to different machine)
  │
  ▼
On success:
  - Encrypt agentToken with DPAPI (Scope::User): windows_dpapi::encrypt(token_bytes, Scope::User)
  - Write ciphertext to %APPDATA%\BrevlyPrint\credential.bin
  - Write config to SQLite: printer_name, enabled_types, tenant_id
  - Register HKCU\Software\Microsoft\Windows\CurrentVersion\Run\BrevlyPrint
    (via auto-launch crate or direct winreg write)
  - Close SetupWindow; agent starts normally
  │
  ▼
Normal startup (subsequent runs):
  - Read credential.bin → DPAPI decrypt → agentToken in memory (String)
  - agentToken used as:
    (a) Bearer token for all HTTP calls (JobFetcher, AckSender, PendingPoller)
    (b) Token sent to Pusher auth endpoint when subscribing private channel
```

**Security properties:**
- DPAPI Scope::User: ciphertext is tied to the Windows user account + machine. Cannot be decrypted on another machine or by another user account.
- agentToken is never stored in plaintext on disk; it lives only in memory after DPAPI decryption at startup.
- The Noren activation endpoint should invalidate serials on re-activation from a different machine (or issue a new token, invalidating the old one).

---

## Agent↔Noren API Contract

These are the endpoints Noren must expose. All are prefixed `/api/agent/` and require `Authorization: Bearer {agentToken}` except activation.

### 1. Activation

```
POST /api/agent/activate
Authorization: none
Body: { serial: string }

200: { agentToken: string, tenantId: string, enabledTypes: ("order"|"dispatch"|"close"|"cancel")[] }
400: { error: "invalid_serial" }
409: { error: "already_activated", boundTo?: string }
```

`agentToken` is a long-lived opaque token (recommend: signed JWT with `sub=tenantId`, `scope=agent`, no expiry or 1-year expiry). Noren stores `serial → tenantId + agentToken` in a `agent_serials` table.

### 2. Pusher Private-Channel Auth

```
POST /api/agent/pusher/auth
Authorization: Bearer {agentToken}
Body: { socket_id: string, channel_name: string }

200: { auth: string }   (Pusher HMAC signature: "appKey:HMAC_SHA256(secret, socketId:channelName)")
401: { error: "unauthorized" }
403: { error: "wrong_tenant_channel" }  (channel doesn't match agent's tenantId)
```

Noren validates that `channel_name` == `private-tenant-{tenantId}-print` where tenantId matches the agentToken. This prevents cross-tenant channel spoofing.

### 3. List Pending Jobs (reconnect pull)

```
GET /api/agent/jobs/pending
Authorization: Bearer {agentToken}

200: { jobs: [ { jobId: string, type: "order"|"dispatch"|"close"|"cancel", createdAt: string } ] }
     sorted by createdAt ASC, max 100 items (paginate via cursor if needed later)
```

Returns all jobs for this tenant that have no ack and are within TTL (e.g. 24h). The agent deduplicates against `printed_jobs` before enqueuing.

### 4. Get Job Bytes

```
GET /api/agent/jobs/{jobId}/bytes
Authorization: Bearer {agentToken}

200: { bytes: string }   (base64-encoded ESC/POS byte sequence)
     Content-Type: application/json
404: { error: "not_found" }    (expired or deleted)
409: { error: "already_printed" }  (acked; agent should mark done and skip)
403: { error: "wrong_tenant" }
```

Noren renders ESC/POS server-side (migrating `buildTicket` etc. from client `src/lib/utils/ticket.ts` to a server-side function). Bytes are returned base64 to stay within JSON; agent decodes to `Vec<u8>` before printing.

### 5. Ack Job

```
POST /api/agent/jobs/{jobId}/ack
Authorization: Bearer {agentToken}
Body: { printedAt: string }   (ISO 8601)

200: { ok: true }
404: { error: "not_found" }
409: { error: "already_acked" }   (idempotent: agent should treat 409 as success)
```

Noren sets `agent_print_jobs.acked_at = printedAt`, removes job from pending list. **Noren must treat repeated ack as 200 or 409 (never 5xx)** — the agent may retry ack after crash.

### 6. Agent Version Check (for auto-update)

```
GET /api/agent/version
Authorization: Bearer {agentToken}

200: { latestVersion: string, downloadUrl: string, sha256: string }
```

Agent polls this nightly; if `latestVersion > current_version` (semver), downloads binary, verifies SHA256, schedules self-replace on next startup.

---

## Config and State on Disk

All persistent state lives under `%APPDATA%\BrevlyPrint\` (per-user, no admin required).

| File / Table | Contents | Format |
|---|---|---|
| `credential.bin` | DPAPI-encrypted agentToken bytes | Raw binary (DPAPI blob) |
| `state.db` (SQLite) | `config` table, `printed_jobs` table, `retry_queue` table | SQLite (`rusqlite`) |

### SQLite Schema

```sql
-- Agent configuration (single row)
CREATE TABLE config (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- Keys: printer_name, tenant_id, enabled_types (JSON array), last_pull_cursor

-- Dedup and status tracking
CREATE TABLE printed_jobs (
  job_id      TEXT PRIMARY KEY,
  job_type    TEXT NOT NULL,
  status      TEXT NOT NULL CHECK(status IN ('pending','printing','done','failed')),
  attempt     INTEGER NOT NULL DEFAULT 0,
  received_at TEXT NOT NULL,
  printed_at  TEXT,
  failed_at   TEXT
);
CREATE INDEX idx_printed_jobs_status ON printed_jobs(status);

-- Retry queue (jobs in 30s backoff window)
CREATE TABLE retry_queue (
  job_id       TEXT PRIMARY KEY REFERENCES printed_jobs(job_id),
  retry_after  TEXT NOT NULL,   -- ISO8601; agent polls this on a ticker
  attempt      INTEGER NOT NULL
);
```

**Startup recovery:** On start, the agent queries `printed_jobs WHERE status = 'printing'` and re-enqueues those jobs (crash recovery). Jobs with `status = 'pending'` are also re-enqueued; they arrive again via pending-pull if the agent was offline.

---

## Threading / Runtime Model in Rust

### Main Thread

```rust
fn main() {
    // 1. Initialize SQLite, decrypt credential, load config
    // 2. Create sync channels for tray updates
    let (tray_tx, tray_rx) = std::sync::mpsc::sync_channel::<TrayUpdate>(16);

    // 3. Spawn tokio runtime on a dedicated OS thread
    let tokio_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async_main(tray_tx));
    });

    // 4. Win32 event loop on main thread (required by tray-icon)
    let tray = TrayIcon::new(/* ... */);
    loop {
        // Poll tray_rx (non-blocking) to update tray color
        if let Ok(update) = tray_rx.try_recv() {
            match update {
                TrayUpdate::Connected => tray.set_icon(GREEN_ICON),
                TrayUpdate::Disconnected => tray.set_icon(RED_ICON),
            }
        }
        // Process Win32 messages
        process_win32_messages(); // GetMessage / TranslateMessage / DispatchMessage
    }
}
```

### Async Core

```rust
async fn async_main(tray_tx: SyncSender<TrayUpdate>) {
    // All components share: Arc<AppState> { db, credential, config }
    let state = Arc::new(AppState::init().await);

    // Spawn independent tasks
    tokio::spawn(event_listener(state.clone(), tray_tx.clone()));
    tokio::spawn(update_checker(state.clone()));
    tokio::spawn(retry_ticker(state.clone()));

    // Block until shutdown signal (ctrl-c or tray "Exit")
    shutdown_signal().await;
}
```

**Channel directions summary:**

| Direction | Channel type | Purpose |
|---|---|---|
| Tokio → Main (tray) | `std::sync::mpsc::SyncSender` | Update tray icon color |
| Main → Tokio | `tokio::sync::oneshot::Sender` | Deliver activation result from GUI |
| Internal async | `tokio::sync::mpsc` | PrintEvent queue, job pipeline |

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Storing agentToken in Plaintext
**What:** Writing the token to a plain text file or Windows Registry string value.
**Why bad:** Any process running as the same user can read it; trivial lateral movement.
**Instead:** DPAPI encrypt (Scope::User) into a `.bin` file. Never log the token.

### Anti-Pattern 2: Calling `block_on` from Inside a Tokio Task
**What:** Calling `tokio::runtime::Handle::current().block_on(...)` inside an `async fn`.
**Why bad:** Panics at runtime ("cannot call block_on inside a runtime").
**Instead:** `await` async functions directly; use `spawn_blocking` only for genuinely blocking sync code (printer I/O is fast enough to not warrant it, but if needed use `tokio::task::spawn_blocking`).

### Anti-Pattern 3: Keeping Dedup State Only in Memory
**What:** Using a `HashSet<String>` in memory for seen job IDs.
**Why bad:** Agent restart (crash, update, Windows reboot) clears the set; jobs printed before restart will print again on the next reconnect/pending-pull.
**Instead:** SQLite `printed_jobs` table with status column. Survives restarts.

### Anti-Pattern 4: Printing from the Fetcher Task
**What:** One big task that fetches bytes AND writes to the printer.
**Why bad:** Printer I/O failure aborts the fetch; harder to retry with backoff; poor separation of concerns.
**Instead:** Separate `JobFetcher` → channel → `PrintWorker` → `RetryScheduler` pipeline.

### Anti-Pattern 5: Passing Raw ESC/POS Through Pusher
**What:** Encoding bytes as base64 in the Pusher event payload.
**Why bad:** Pusher event limit is ~10 KB; closing receipts can easily exceed this.
**Instead:** Pusher delivers only `{ jobId, type }`; bytes come via authenticated HTTP GET.

---

## Suggested Build Order (Component Dependencies)

Build order follows dependency graph: each phase's components only depend on already-built components.

```
Phase 1 — Foundation
  ConfigStore (SQLite schema, read/write config)
  CredentialStore (DPAPI encrypt/decrypt)
  No dependencies.

Phase 2 — Activation Flow
  SetupWindow (native-windows-gui dialog)
  Activation HTTP call (reqwest POST /activate)
  Autostart registration (winreg / auto-launch)
  Depends on: ConfigStore, CredentialStore

Phase 3 — Tray + Runtime Bridge
  Win32 message loop skeleton
  TrayIcon (green/red)
  Tokio runtime thread spawn
  Sync channel bridge (TrayUpdate)
  Depends on: Phase 1

Phase 4 — Pusher Integration
  EventListener (pusher_rs or raw WS + Pusher protocol)
  Private channel auth (POST /pusher/auth)
  PrintEvent → JobQueue channel
  Depends on: Phase 3, CredentialStore

Phase 5 — Job Pipeline
  JobFetcher (GET /jobs/{id}/bytes)
  PrintWorker (USB via escpos+usbprint.sys, serial via serialport)
  AckSender (POST /jobs/{id}/ack)
  Dedup check against SQLite
  Depends on: Phase 4, ConfigStore

Phase 6 — Resilience
  RetryScheduler (3× / 30s, status tracking in SQLite)
  PendingPoller (GET /jobs/pending on reconnect)
  Windows notification on final failure
  Depends on: Phase 5

Phase 7 — Auto-Update
  UpdateChecker (GET /agent/version, self_update crate, sha256 verify)
  Depends on: Phase 3 (runtime), CredentialStore
```

**Critical path:** Phases 1 → 2 → 3 → 4 → 5 must be sequential. Phase 6 can be built alongside Phase 5 (resilience wraps the pipeline). Phase 7 is fully independent after Phase 3.

---

## Technology Decisions

| Component | Crate / API | Rationale |
|---|---|---|
| Tray icon | `tray-icon` (tauri-apps) | Actively maintained; handles taskbar restart; Win32-native; `Send + Sync` compatible with async |
| Activation dialog | `native-windows-gui` | Native Win32 dialog; no webview; minimal binary footprint |
| Async runtime | `tokio` (multi-thread) | Standard; required by reqwest and pusher_rs |
| Pusher WS client | `pusher_rs` (lib.rs) | Supports private channels, exponential backoff reconnect; Pusher protocol v7 |
| HTTP client | `reqwest` | Async, TLS, widely used; JSON via serde_json |
| USB printing | `escpos` crate + `usbprint.sys` | Uses standard Windows kernel driver via CreateFile/WriteFile; no Zadig/WinUSB required |
| Serial printing | `serialport` crate | Cross-platform serial; Windows COM ports |
| Credential storage | `windows-dpapi` crate | Safe Rust wrapper; Scope::User encryption tied to user+machine |
| Autostart | `auto-launch` crate | Writes HKCU Run key; no admin required |
| State / dedup | `rusqlite` (bundled) | Single-file SQLite; no external server; transactional dedup; survives restarts |
| Self-update | `self_update` crate | Replaces running binary on Windows (uses self-replace under the hood); supports SHA256 verify |

---

## Sources

- tray-icon thread requirements: https://docs.rs/tray-icon/latest/tray_icon/
- Tokio bridging sync/async: https://tokio.rs/tokio/topics/bridging
- windows-dpapi crate: https://crates.io/crates/windows-dpapi / https://github.com/sheridans/windows-dpapi
- Pusher private channel auth protocol: https://pusher.com/docs/channels/server_api/authorizing-users/
- Pusher Channels WebSocket protocol: https://pusher.com/docs/channels/library_auth_reference/pusher-websockets-protocol/
- pusher_rs Rust client: https://lib.rs/crates/pusher-rs
- escpos crate (usbprint.sys): https://lib.rs/crates/escpos
- Microsoft usbprint.sys docs: https://learn.microsoft.com/en-us/windows-hardware/drivers/print/usb-printing
- self_update crate: https://github.com/jaemk/self_update
- self-replace crate (Windows binary swap): https://docs.rs/self-replace
- auto-launch crate: https://crates.io/crates/auto-launch
- Webhook idempotency patterns: https://www.averagedevs.com/blog/reliable-webhook-delivery-idempotent-secure
