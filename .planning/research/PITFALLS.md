# Domain Pitfalls

**Project:** Brevly Print — Rust native Windows thermal print agent
**Domain:** ESC/POS thermal printing + Pusher WebSocket + Windows deployment
**Researched:** 2026-07-15

---

## Critical Pitfalls

Mistakes that cause rewrites, data loss, or complete field failures.

---

### Pitfall C1: RAW Datatype Not Specified — Driver Mangles ESC/POS Bytes

**What goes wrong:** When using `WritePrinter` via the Windows spooler, the `pDatatype` field of `DOC_INFO_1W` must be `"RAW"`. If omitted or set to `"TEXT"`, the spooler routes bytes through the installed printer driver's rendering pipeline — the driver interprets ESC/POS control bytes as text, converts line endings, strips or corrupts binary sequences, and the printer emits garbled output or nothing at all.

**Why it happens:** The Windows spooler supports multiple datatypes (`RAW`, `TEXT`, `XPS_PASS`, etc.). The default when unspecified is driver-determined. Many thermal printer drivers installed from manufacturer CDs ship as v3/v4 drivers that use XPS-based pipelines; sending `RAW` to an XPS driver produces a 0-byte spool file. Conversely, sending `TEXT` to a raw-capable driver converts CR+LF and tries to interpret bytes as encoding — corrupting all binary ESC/POS sequences.

**Consequences:** Printer prints garbage, blank receipts, or nothing. Spool job may "succeed" (return value non-zero from `WritePrinter`) while the printer output is wrong, making it undetectable at the call site. Operator sees empty paper and blames the software.

**Prevention:**
- Always set `pDatatype: "RAW"` in `DOC_INFO_1W` with no exceptions.
- After `StartDocPrinterW` returns a job ID, check it is nonzero before writing bytes.
- During the setup flow (Phase 2), validate that the selected printer accepts RAW jobs: call `OpenPrinterW`, attempt a `StartDocPrinterW` with `"RAW"`, check for failure, and surface a friendly error if the installed driver does not support it.
- If the printer was installed with a manufacturer v4/XPS driver, document in the setup guide that the user should add the printer as **Generic / Text Only** instead — this driver sends raw bytes without processing.
- Add a test-print button in the activation window (Phase 2) that sends a known minimal ESC/POS sequence (`ESC @` reset + `GS V \x41 \x00` full cut) so the operator can confirm bytes arrive correctly before the agent goes live.

**Detection:** Blank or garbled output despite `WritePrinter` returning success. Use `GetJob` to inspect the spool file size after `EndDocPrinter`; a 0-byte spool file indicates the driver ate the data.

**Phase:** Phase 5 (PrintWorker USB path) — enforce `"RAW"` datatype. Phase 2 (SetupWindow) — add test-print validation.

---

### Pitfall C2: eframe Owns the Main Thread — Conflicts with tao + tray-icon

**What goes wrong:** Both `eframe::run_native()` (egui) and `tao::EventLoop::run()` block the calling thread and own the Win32 message loop. On Windows, Win32 windows can only be created on the thread that owns the message loop. You cannot run two event loops on the same thread, and you cannot move event loops to background threads — `EventLoop` is `!Send` on non-Unix platforms. Trying to put `tao` on main and `eframe` on a worker thread, or vice versa, causes panics or silently broken window behavior.

**Why it happens:** `tray-icon` requires its `tao::EventLoop` on the main OS thread. `eframe` also calls `EventLoop::new()` internally when `run_native()` is called. On Windows, winit (which both tao and eframe use internally) enforces that only one `EventLoop` can exist per process and that it must be created on the main thread.

**Consequences:** Application panics on startup (`EventLoop::new()` called from non-main thread`), or the tray icon stops updating after the activation window closes, or the activation window never appears.

**Prevention:**
- Do **not** use `eframe::run_native()` for the activation window — this API owns the event loop and is incompatible with `tao`.
- Instead, render the activation window by embedding raw egui rendering into the tao event loop: use `egui` + `egui-wgpu` (or `egui-glow`) directly, creating the window via `tao::window::WindowBuilder` and driving the paint loop from within `tao`'s `EventLoop::run()` callback.
- Alternatively (simpler path): Use a separate OS process for the setup window, spawning it on activation trigger and communicating result back via a local pipe or temp file. The agent main process only runs the tao loop; setup is a second short-lived binary.
- The confirmed approach from STACK.md (use `egui` via `eframe`) requires this architecture resolution as a **Phase 1 spike before any GUI code is written**.
- Flag this as a required spike in Phase 1 (Foundation). The tao+egui integration thread model must be proven before committing to either crate for the activation window.

**Detection:** `thread 'main' panicked at 'EventLoop::new() can only be called from the main thread'` — or tray icon freezes after setup window opens.

**Phase:** Phase 1 spike (prove the thread model) → Phase 2 (SetupWindow implementation must use the validated approach).

---

### Pitfall C3: Dedup State Lost on Crash — Reprints on Reconnect

**What goes wrong:** If job dedup lives only in a `HashSet<String>` in memory, any agent crash, update, or Windows reboot clears the set. On next startup + Pusher reconnect, the `PendingPoller` fetches all unacked jobs — including jobs that were printed successfully before the crash. Those jobs print again.

**Why it happens:** The natural first-pass implementation is in-memory dedup. The reconnect pending-pull exists specifically to cover offline scenarios, making it impossible to distinguish "printed before crash" from "never received" without persistent state.

**Consequences:** Customer receives duplicate receipts for orders; cashier reprints all shift tickets after a reboot. For a restaurant context, duplicate kitchen tickets cause confusion and wasted food.

**Prevention:**
- SQLite `printed_jobs` table with status column is the only correct solution. This is already in the confirmed architecture (ARCHITECTURE.md `printed_jobs` schema).
- Status `done` must be written **before** ack is sent, in the same SQLite transaction as the print success record. If the agent crashes after print but before writing `done`, the job will be re-fetched and re-printed on restart — this is acceptable only if the ESC/POS bytes are truly idempotent (they are for thermal receipts: duplicate print = duplicate paper = operator discards one).
- On startup, re-enqueue any row with `status = 'printing'` (crash recovery path). These are jobs where the print was in-flight when the process died.
- Never use `status = 'printing'` as a dedup fence — only `done` guarantees the job was printed.

**Detection:** Operator reports receiving two identical receipts for one order after the PC was restarted. `printed_jobs` table missing entries for recently printed jobs.

**Phase:** Phase 5 (JobFetcher + PrintWorker) — enforce SQLite status write before ack. Phase 1 (Foundation) — schema must exist from the start with correct indexes.

---

### Pitfall C4: Ack Sent Before Print Confirmed — Lost Tickets on Crash

**What goes wrong:** The inverse of C3: acking the job (POST `/jobs/{id}/ack`) before `WritePrinter` returns success. If the agent crashes between sending the ack and the printer writing the bytes, Noren removes the job from its pending queue. On reconnect, `PendingPoller` no longer returns the job. The ticket is silently lost — never printed, never retried.

**Why it happens:** Developers optimistically ack to reduce latency, not realizing the ack constitutes a delivery guarantee from the server's perspective.

**Consequences:** Lost kitchen ticket. Order never prepared. Customer waits indefinitely.

**Prevention:**
- The ack call (POST `/jobs/{id}/ack`) must happen in `AckSender` only after `PrintWorker` has confirmed bytes written to the printer AND SQLite status has been updated to `done`.
- The pipeline is: `PrintWorker` success → write `done` to SQLite → send job to `AckSender` channel → `AckSender` POSTs ack.
- If ack POST itself fails (network error), the job stays `done` in SQLite but unacked on the server. On next startup, `PendingPoller` returns the job again; dedup check finds `status = 'done'` → discard → re-send ack. Noren must accept repeated acks (409 Already Acked → treat as 200). This is documented in the API contract.
- Never batch-ack or fire-and-forget from `PrintWorker` directly.

**Detection:** Operator reports ticket was not printed but Noren shows it as "printed". The `printed_jobs` row will be missing for the lost job.

**Phase:** Phase 5 (AckSender) — enforce ordering. Phase 4 (Noren API contract) — server must return 409 (not 500) on repeated ack.

---

### Pitfall C5: Pusher Silent Disconnection After >5 Minutes Offline

**What goes wrong:** After a network outage of more than approximately 5 minutes, Pusher's WebSocket connection can enter a "zombie" state: the TCP connection is closed at the OS level, but the WebSocket client reports `readyState === OPEN` and fires no `onclose` event. The client believes it is connected; Pusher has timed out the connection on its side. No new events are delivered. No `pusher:connection_established` fires. The agent's tray icon stays green while silently missing all print jobs.

**Why it happens:** WebSocket connections depend on TCP keep-alive or application-level ping/pong to detect dead connections. Without periodic pings, a NAT or firewall can silently drop the idle TCP stream while neither endpoint detects the closure. Pusher's protocol specifies a `pusher:ping` / `pusher:pong` mechanism for exactly this purpose, but only client libraries that implement it correctly will detect silent drops.

**Consequences:** Agent appears healthy (green tray), but no Pusher events are received. Jobs are silently lost until the operator notices nothing is printing. The `PendingPoller` only runs on reconnect — if reconnect never fires, pending jobs are never pulled.

**Prevention:**
- The hand-rolled Pusher client (over `tokio-tungstenite`) must implement application-level ping/pong: send `{"event":"pusher:ping","data":{}}` at a configurable interval (30s is appropriate) and await `pusher:pong` within a timeout (10s). If pong is not received, close the socket explicitly and trigger reconnect.
- Use `tokio::time::interval` for the ping ticker and `tokio::time::timeout` to gate on the pong response.
- On reconnect, always re-run `PendingPoller` regardless of how the disconnect was detected (explicit close vs. ping timeout) so no events are missed during the zombie window.
- Track `last_event_received_at` in memory; if no event or pong has been received in >90 seconds while the socket reports connected, force-reconnect.
- Set `tray → yellow` on ping timeout before reconnect attempt so the operator can see the agent is aware of the issue.

**Detection:** Tray is green but no jobs are printing despite orders being placed. Agent logs show no Pusher messages received for >5 minutes.

**Phase:** Phase 4 (EventListener) — ping/pong loop is mandatory from day one, not an optimization.

---

### Pitfall C6: Cross-Tenant Channel Subscription Not Validated Server-Side

**What goes wrong:** The Pusher auth endpoint (`POST /api/agent/pusher/auth`) validates the `agentToken` but does not verify that the requested `channel_name` matches the agent's `tenantId`. A compromised or cloned agent with a valid token could request auth for another tenant's channel (`private-tenant-OTHER_ID-print`) and receive real-time print events belonging to another restaurant.

**Why it happens:** The `agentToken` proves the agent is legitimate, but without channel-name verification, any legitimate agent can subscribe to any channel. Pusher delegates all private-channel authorization to your server; if your server doesn't check, Pusher doesn't either.

**Consequences:** Tenant A's agent can subscribe to Tenant B's print channel, receiving order data (customer addresses, item details) — a data breach.

**Prevention:**
- The `/api/agent/pusher/auth` endpoint on Noren must verify: `channel_name === private-tenant-${tenantId}-print` where `tenantId` is extracted from the authenticated `agentToken`. Return `403` if it does not match.
- This is documented in the API contract (ARCHITECTURE.md §2) — ensure it is implemented as a non-bypassable check, not a log-only warning.
- The agent should only ever request subscription to `private-tenant-${ownTenantId}-print`; any other channel request from the agent side indicates a bug.

**Detection:** A Noren audit log shows auth requests for channels that do not match the agent's tenant. Code review of `/api/agent/pusher/auth` handler.

**Phase:** Phase 4 (Pusher auth endpoint on Noren) — must be implemented correctly before any real tenant data flows.

---

## Moderate Pitfalls

Mistakes that cause field pain and support calls but do not require rewrites.

---

### Pitfall M1: USB Thermal Printer Appears as Virtual COM Port (Not Windows Printer)

**What goes wrong:** Some thermal printers (particularly cheaper Chinese 80mm models sold in Brazil) use a USB-Serial bridge chip (CH340, PL2303, CP210x) instead of usbprint.sys. These appear in Device Manager as `COM3` (or similar), not as an installed Windows printer. The `printers` crate (`EnumPrintersW`) returns nothing for them. If the setup UI only offers "installed Windows printers" in the dropdown, these devices are invisible.

**Why it happens:** Two USB-to-printer implementations exist in the market: (a) USB Printer Class (usbprint.sys, appears as Windows printer) and (b) USB-to-Serial adapter (appears as COMx). Epson TM-T20X uses usbprint.sys. Generic 80mm models commonly use the serial bridge.

**Consequences:** Setup flow fails silently — printer dropdown is empty. Operator cannot activate the agent. Support call required.

**Prevention:**
- The setup dropdown must offer **both** sources: enumerate installed Windows printers via `printers` crate AND enumerate available COM ports via `serialport::available_ports()`.
- Display them in two groups: "Impressoras instaladas" and "Portas seriais (COM)".
- The selected entry type (printer name vs. COMx) determines which print path (`WritePrinter` vs. `serialport`) the `PrintWorker` uses at runtime.
- This printer-type selection is stored in the `config` table (e.g., `printer_type: "windows_printer" | "serial_port"`).

**Detection:** Setup dropdown is empty despite printer being connected and powered. Device Manager shows COMx under "Ports" for the printer.

**Phase:** Phase 2 (SetupWindow) — enumerate both sources. Phase 5 (PrintWorker) — two print paths gated on stored `printer_type`.

---

### Pitfall M2: Paper-Out / Offline Detection Is Opaque via WritePrinter

**What goes wrong:** `WritePrinter` returns a non-zero byte count (apparent success) even when the printer is out of paper or in an error state, because the spooler has buffered the bytes. The actual printer status error only surfaces later, if at all. `GetJob` can return job status `JOB_STATUS_ERROR` eventually, but the timing is unpredictable and the error code does not distinguish "out of paper" from "cover open" from "offline".

**Why it happens:** The Windows print spooler decouples the write call from actual printer communication. The spooler accepts bytes into its queue immediately. USB bulk-transfer errors from usbprint.sys bubble up asynchronously. The granularity of `PRINTER_STATUS_*` flags (`GetPrinter` level 2) is coarse: `PRINTER_STATUS_PAPER_OUT`, `PRINTER_STATUS_OFFLINE`, `PRINTER_STATUS_ERROR` are the main flags, but not all printer drivers set them correctly.

**Consequences:** `PrintWorker` reports success; retry is never triggered; the ticket is never printed; no notification reaches the operator; ack is sent to Noren. Job is silently lost.

**Prevention:**
- After `EndDocPrinter`, poll `GetPrinter` (level 2) with a 1-second delay for up to 5 seconds. Check `PRINTER_STATUS_PAPER_OUT`, `PRINTER_STATUS_OFFLINE`, `PRINTER_STATUS_ERROR`. If any error flag is set, treat the job as failed and route to `RetryScheduler`.
- This is imperfect — polling window is heuristic. Document this limitation: USB printer status is best-effort, not guaranteed.
- For serial-connected printers, send `DLE EOT n=4` (paper roll sensor status) before printing to proactively detect paper-out. Parse the returned status byte: bit 5 set = paper-end sensor triggered. This is available on Epson-compatible printers; availability on clone models varies.
- Surface the detected state in the tray tooltip: "Impressora: sem papel" vs. "Impressora: offline" vs. "Impressora: erro" — even if the error code is the same, the tooltip text can be updated heuristically.
- Never send ack until polling confirms no error flags, or until the timeout elapses without error (positive indication is absence of error flags, not a positive "OK" signal).

**Detection:** Job marked `done` in SQLite and acked on server, but no paper emerged from printer. Printer LED shows error state.

**Phase:** Phase 5 (PrintWorker) — post-write status poll. Phase 6 (Resilience) — integrate error detection with RetryScheduler.

---

### Pitfall M3: Partial-Cut Command Variance Across Printer Models

**What goes wrong:** `GS V \x42 \x00` (partial cut with n=0 feed) is the most widely supported cut command, but behavior varies by model. Some printers treat all `GS V` parameters as full cut. Some only cut on even/odd parameter values. Some require a minimum line feed before cutting or the cutter jams. Clone printers sometimes ignore `GS V` entirely and only respond to `ESC i` (Epson legacy full cut).

**Why it happens:** The ESC/POS specification is a de facto standard, not a formal one. Epson's implementation is the reference, but clone manufacturers implement subsets or deviations.

**Consequences:** Paper does not cut, or cuts in the wrong position, leaving tickets attached to the roll. The operator must manually tear. With high order volume, the roll jams because uncut tickets pile up at the cutter.

**Prevention:**
- Since Noren renders the ESC/POS bytes server-side, the cut command is defined in `buildTicket` / `buildDespachoTicket`. Document this as the single point of configuration.
- Default to `GS V \x42 \x00` (partial cut, 0 additional lines). Precede it with 4+ blank `LF` lines to ensure content clears the cutter position.
- If customer reports cut issues, the server-side template can be patched for their tenant's printer model — no agent update required (this is the spooler-burro benefit).
- For setup validation, the test-print sequence should include a cut command and the operator should confirm paper cuts.

**Detection:** Paper roll does not separate after print; operator complains tickets are joined.

**Phase:** Phase 5 (PrintWorker) is unaffected — bytes arrive from Noren. Document the configuration point for Noren's Phase 1 (server-side ESC/POS rendering migration).

---

### Pitfall M4: Pusher Socket ID Changes on Reconnect — Auth Must Be Re-Requested Per Connection

**What goes wrong:** The Pusher private-channel auth signature is tied to `socket_id`. On every reconnect, Pusher assigns a new `socket_id`. If the hand-rolled client reuses the previous auth string (cached from the previous connection), the `pusher:subscribe` message is rejected with `pusher:subscription_error` and the channel is never subscribed — silently missing all events.

**Why it happens:** Pusher's auth format is `HMAC_SHA256(appSecret, "${socketId}:${channelName}")`. The `socketId` is bound to a single WebSocket session. Reconnecting creates a new session with a new `socketId`, invalidating all previous auth tokens.

**Consequences:** After any disconnect+reconnect cycle, the agent appears connected (WebSocket handshake succeeds, `pusher:connection_established` fires) but is not subscribed to any channel. Events are missed indefinitely until the next full process restart.

**Detection:** Tray shows green (connected), but no events are received after a reconnect. Logs show `pusher:connection_established` but no subsequent `pusher:subscription_success`.

**Prevention:**
- The `EventListener` reconnect path must always re-POST `/api/agent/pusher/auth` with the new `socket_id` before attempting to subscribe. Never cache auth strings.
- After receiving `pusher:connection_established` (which contains `socket_id`), immediately POST auth with the new `socket_id`, then send subscribe with the fresh auth string.
- Write a reconnect integration test that verifies subscription succeeds after simulating a disconnect.

**Phase:** Phase 4 (EventListener reconnect path) — auth re-request is part of the reconnect sequence, not an afterthought.

---

### Pitfall M5: SmartScreen Blocks Installer for New Users — Undocumented Reputation Threshold

**What goes wrong:** Even with an OV Authenticode signature, Windows SmartScreen shows "Windows protected your PC" on first download for new software. Since March 2024, EV certificates no longer grant instant reputation bypass. The threshold for clearing the warning is not published by Microsoft — anecdotally, hundreds of clean installs over time. A restaurant owner who sees this warning and cannot find "More info → Run anyway" will call it a virus and uninstall.

**Why it happens:** SmartScreen assigns reputation scores to file hashes. A new binary with no history defaults to "unknown" regardless of signing. The OV certificate only prevents the "This file is unsigned" hard block; it does not grant trust.

**Consequences:** Adoption friction at every restaurant. Non-technical owners hit an unfamiliar security prompt and abandon the install.

**Prevention:**
- Sign the installer with an OV (or EV) Authenticode certificate from day one — unsigned binaries get a harder block than the SmartScreen "unknown publisher" warning.
- Prepare installation instructions that include a screenshot showing "More info → Run anyway" for the initial period.
- Submit the binary to Microsoft's Defender Intelligence portal for manual review (no guarantee, but accelerates clean-file reputation).
- Use a consistent download URL (do not rotate domains) so download count accrues to one origin.
- Consider distributing through a known channel (direct link from Noren dashboard) rather than a generic file host — download source reputation influences SmartScreen scoring.
- Plan for 2–6 weeks of warnings for early adopters; communicate this expectation proactively.

**Detection:** Beta tester reports "Windows protected your PC" prompt on every fresh install.

**Phase:** Phase 7 (Auto-Update / Installer) — sign from the first release, not as an afterthought. The signing setup (certificate procurement, signtool pipeline in CI) should be configured in Phase 3 when the first distributable binary is built.

---

### Pitfall M6: Auto-Update Replaces Locked Running EXE on Windows

**What goes wrong:** On Windows, a running EXE is locked. The typical `self_update` approach of downloading a new binary and overwriting the current path fails with `ERROR_SHARING_VIOLATION`. The update silently fails, no error is surfaced to the operator, and the agent continues running the old version indefinitely.

**Why it happens:** Windows file locking semantics differ from Unix: you can `rename` a running EXE but cannot `unlink` it. The `self_update` crate handles this via `self-replace` (rename old to `.old`, move new to the original path), but the old file cannot be deleted until the process exits — and if the rename step fails (e.g., antivirus holds a read lock during scan), the whole update fails silently.

**Consequences:** Agent runs perpetually outdated. Security patches and bug fixes are not applied. Operators do not know they are out of date.

**Prevention:**
- Use **Velopack** (`velopack` crate) as the update mechanism. Velopack uses a bootstrapper process (`Update.exe`) that applies the update after the agent exits — the running EXE is never touched while the process is alive. This is the architecture that correctly handles Windows file locking.
- The agent's update flow: `UpdateChecker` finds new version → downloads delta package to temp → schedules update flag → on next Windows login (or on graceful "restart for update" command), Velopack's bootstrapper runs before the agent binary and performs the swap.
- Never attempt an in-place binary replacement while the process is running.
- If Velopack Rust SDK proves immature (noted as MEDIUM confidence in STACK.md), the fallback is: download new binary to `%APPDATA%\BrevlyPrint\update\BrevlyPrint-new.exe`, register a `RunOnce` registry key that moves it to the install path on next boot, and log the pending update in SQLite. This avoids any locked-file problem at the cost of losing delta packages.
- Verify update payload SHA256 before scheduling it, regardless of update mechanism.

**Detection:** Agent version does not increment after several auto-update cycles. SQLite `update_check` log shows download success but version remains stale.

**Phase:** Phase 7 (UpdateChecker) — spike Velopack Rust SDK first to confirm it handles the Windows bootstrap flow correctly before committing.

---

### Pitfall M7: DPAPI User Scope Breaks If User Profile Changes

**What goes wrong:** DPAPI `Scope::User` ties ciphertext to the Windows user account's master key. If the restaurant PC undergoes a Windows reinstall, the user profile is recreated with a different master key. `credential.bin` (the encrypted agent token) cannot be decrypted by the new profile. The agent silently fails to decrypt and either crashes or shows the activation window again — but the serial may already be bound to the previous machine/user, causing `409 already_activated`.

**Why it happens:** DPAPI Scope::User encryption is tied to the Windows user's derived key, which is derived from the user's password and SID. A new Windows install creates a new SID even if the username is the same.

**Consequences:** Agent becomes unusable after a Windows reinstall without a re-activation flow. If the serial is single-use (bound to one machine), the owner cannot re-activate without support intervention.

**Prevention:**
- The Noren activation endpoint must support re-activation: if the same serial activates again from a different machine, invalidate the old token and issue a new one (not return `409` permanently). Log both activations for audit.
- The agent must gracefully handle DPAPI decryption failure: catch the error, delete `credential.bin` and the `config` SQLite database, and re-enter the activation flow with a clear message: "Credenciais expiradas — reative o agente."
- Avoid `Scope::Machine` as an alternative — it would allow any process on the machine to read the token, defeating the security model.
- This is a support-burden design decision: document it and build the re-activation recovery path in Phase 2 (activation error handling) rather than discovering it in production.

**Detection:** Agent shows activation window on a PC that was previously activated. DPAPI decryption returns `ERROR_DECRYPTION_FAILED`.

**Phase:** Phase 1 (CredentialStore) — handle decryption failure gracefully. Phase 2 (SetupWindow) — surface clear error message. Noren API — support re-activation.

---

### Pitfall M8: Task Manager Startup Disable Silently Prevents Agent from Starting

**What goes wrong:** Windows 10/11 Task Manager's Startup tab allows users to disable startup apps. When a user (or the IT help in a restaurant) disables "BrevlyPrint" in Task Manager, the agent stops starting with Windows. The `HKCU\Run` registry key still exists, but `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run\BrevlyPrint` is set to a binary value indicating disabled. The agent does not know it was disabled.

**Why it happens:** The `StartupApproved` key was added in Windows 8 to give users control over startup items without deleting the Run key. Many developers only check for the Run key and miss the approval key.

**Consequences:** After a reboot, the agent is not running. Orders arrive but no tickets print. The tray icon is absent. The operator calls support.

**Prevention:**
- The `auto-launch` crate (0.5.x) already handles `StartupApproved` correctly when calling `enable()` and `is_enabled()`. Rely on its implementation rather than raw `winreg` writes.
- During startup health check (on each launch), verify `auto_launch.is_enabled()` returns `true`. If it was disabled by the user, the agent was intentionally stopped — surface this in the Noren dashboard as "agente offline" rather than silently missing prints.
- Document to restaurant operators: do not disable "BrevlyPrint" in Task Manager. Include this in the installation guide.
- The tray context menu "Iniciar com o Windows" should toggle the `auto-launch` state and confirm the current status.

**Detection:** Agent not running after reboot despite being previously active. Task Manager → Startup shows "BrevlyPrint" as Disabled.

**Phase:** Phase 2 (Activation / autostart registration) — use `auto-launch` crate, not raw registry write. Phase 3 (Tray) — add startup status check on launch.

---

### Pitfall M9: Antivirus Quarantines the Agent Binary

**What goes wrong:** Rust binaries that open network connections, write to the registry, and interact with printer APIs exhibit behavioral signatures similar to RAT (Remote Access Trojan) malware. Windows Defender and third-party AV products (common in restaurant chains using managed security) flag Rust-compiled binaries with heuristic ML detections like `Trojan:Win32/Wacatac.B!ml`. The binary is quarantined automatically without any user prompt, and the agent silently stops running.

**Why it happens:** AV heuristics look at process behavior: persistent background process + network connections + registry writes + printer access = high suspicion score. Rust's small binary size and unusual PE structure (compared to .NET/Electron apps) increases false-positive rates from ML-based detectors. This is a documented issue across Tauri, RustDesk, and other Rust desktop applications.

**Consequences:** Agent disappears from the tray with no warning. Orders are lost. The operator does not know the binary was quarantined (AV notification may be silent on locked-down machines).

**Prevention:**
- Authenticode signing reduces (but does not eliminate) AV false positives — sign from day one.
- Submit the release binary to VirusTotal before each distribution and monitor the scan results. Submit to AV vendors' false-positive reporting portals (Microsoft, Kaspersky, ESET) for each detected hash.
- Avoid behavioral patterns that trigger heuristics unnecessarily: do not enumerate all processes, do not scan the filesystem, do not write to unusual registry locations beyond `HKCU\Run`.
- Add a Noren dashboard indicator: "Agente online / offline" based on the last heartbeat ping (the agent can POST a heartbeat to `/api/agent/heartbeat` every 5 minutes). If the agent goes silent, Noren can alert the restaurant owner — this is the only way to detect quarantine since the agent itself cannot report it.

**Detection:** Agent disappears from the tray. Windows Security → Protection History shows quarantined item. VirusTotal scan of the binary shows detections.

**Phase:** Phase 3 (first distributable build) — set up VirusTotal scan in CI. Phase 6 (Resilience) — implement heartbeat endpoint for Noren dashboard visibility.

---

## Minor Pitfalls

---

### Pitfall m1: Toast Notification App ID Not Registered — Toasts Appear as "PowerShell"

**What goes wrong:** Windows toast notifications require an `AppUserModelId` registered in the Start Menu or registry (`SOFTWARE\Classes\AppUserModelId\BrevlyPrint`) to display correctly. Without it, `tauri-winrt-notification` falls back to the PowerShell App ID. The toast appears to come from "Windows PowerShell", confusing the operator and making it harder to identify the source of the print-failure alert.

**Prevention:** Register the AppUserModelId during installation (Velopack installer handles this automatically). If using a custom installer, add the registry entry. Verify by checking that `Toast::new("BrevlyPrint")` shows the correct app name and icon in the notification.

**Phase:** Phase 3 (Tray) — toast is first used for status. Installer must register AppUserModelId.

---

### Pitfall m2: SQLite File Opens Fail if %APPDATA% Directory Not Created

**What goes wrong:** `rusqlite::Connection::open("%APPDATA%\\BrevlyPrint\\state.db")` returns `SQLITE_CANTOPEN` if the `BrevlyPrint` directory does not exist. On first run, before activation, the directory does not exist.

**Prevention:** Call `std::fs::create_dir_all(app_data_dir)` before any SQLite or file operations. Do this as the first action in `AppState::init()`.

**Phase:** Phase 1 (Foundation / ConfigStore init).

---

### Pitfall m3: ISO-8859-1 Encoding Drift — Accented Characters Garbled

**What goes wrong:** ESC/POS commands for encoding selection (`ESC t n`) select a codepage (e.g., codepage 2 = PC850 Multilingual, codepage 16 = WPC1252). If the Noren server-side ESC/POS renderer does not set the correct codepage command before text, or if the byte sequence for `ã`, `ç`, `é` is generated in UTF-8 rather than the printer's active codepage, characters appear garbled.

**Why it matters here:** Noren's existing `ticket.ts` already uses ISO-8859-1 encoding. When migrating to server-side rendering, the encoding logic must be preserved exactly — not silently converted to UTF-8 string handling in Node.js/SvelteKit.

**Prevention:** The Noren server-side migration of `buildTicket` must test character encoding with real printer output, not just byte comparison. Specifically test: `ã`, `ç`, `é`, `ó`, `ú`, `Ç` in item names and address fields.

**Phase:** Noren Phase 1 (server-side ESC/POS migration) — outside this agent's scope, but flag it.

---

### Pitfall m4: Retry Queue and PendingPoller Race Condition

**What goes wrong:** A job in `RetryScheduler` (waiting 30s for retry attempt 2) and the same job in the `PendingPoller` result (because it is not yet acked) could both attempt to enqueue the same `jobId`. The dedup check handles this correctly only if the SQLite read-and-insert is atomic.

**Prevention:** Use a SQLite `INSERT OR IGNORE INTO printed_jobs` (or equivalent upsert) as the dedup gate. Since `job_id` is the PRIMARY KEY, concurrent attempts to insert the same ID result in only one succeeding. Wrap the check+insert in a transaction.

**Phase:** Phase 5 (JobFetcher dedup gate) — use `INSERT OR IGNORE`, not a `SELECT` followed by `INSERT`.

---

### Pitfall m5: Pusher Connection Limit and Per-Tenant Channel Scoping

**What goes wrong:** Pusher free/starter plans have connection limits (e.g., 100 concurrent connections). Each agent = 1 connection. At 100+ restaurants, the limit is hit. Additionally, if channel names are not scoped to `tenantId` (e.g., using a generic `private-print-jobs` channel), all agents receive all print events — O(n) spurious events per job.

**Prevention:** Channel name must always include `tenantId` (`private-tenant-${tenantId}-print`). This is already in the architecture. Monitor Pusher connection count as the customer base grows; upgrade plan or switch to Pusher Beams / self-hosted Soketi at scale.

**Phase:** Phase 4 (Pusher integration). Noren billing review at ~50 restaurants.

---

## Phase-Specific Warning Summary

| Phase | Topic | Likely Pitfall | Mitigation |
|-------|-------|---------------|------------|
| Phase 1 | SQLite init | Directory not created → `SQLITE_CANTOPEN` | `create_dir_all` before `Connection::open` |
| Phase 1 | CredentialStore | DPAPI decryption failure after profile change | Graceful error → re-activation flow |
| Phase 1 | Thread model spike | eframe + tao event loop conflict | Prove integration before writing GUI code |
| Phase 2 | SetupWindow | USB printer appears as COMx, not Windows printer | Enumerate both sources in dropdown |
| Phase 2 | Autostart | `StartupApproved` key disables silent | Use `auto-launch` crate, verify on each launch |
| Phase 2 | Re-activation | DPAPI decrypt fails after Windows reinstall | Clear creds + re-enter activation flow |
| Phase 3 | Installer | SmartScreen blocks unsigned/new binary | Sign from first release; submit to Defender portal |
| Phase 3 | Toast notification | AppUserModelId not registered | Register in installer; verify toast shows correct app name |
| Phase 4 | EventListener | Silent disconnect after >5min offline | Implement ping/pong loop; force reconnect on timeout |
| Phase 4 | Pusher auth | Auth string cached across reconnects | Re-POST auth with new `socket_id` on every reconnect |
| Phase 4 | Channel scoping | Cross-tenant event leakage | Server must validate channel name matches agentToken.tenantId |
| Phase 5 | PrintWorker | RAW datatype not set → driver mangles bytes | Always `pDatatype = "RAW"` in `DOC_INFO_1W`; test-print in setup |
| Phase 5 | AckSender | Ack sent before print confirmed → lost ticket | Ack only after `done` written to SQLite |
| Phase 5 | Dedup | Reprints on reconnect if dedup is in-memory | SQLite `printed_jobs` is the dedup source of truth |
| Phase 5 | Status detection | `WritePrinter` success ≠ paper printed | Poll `GetPrinter` status after write; serial DLE EOT for COM path |
| Phase 5 | Cut commands | `GS V` variance across models | Server-side template is the fix point; test-print includes cut |
| Phase 6 | RetryScheduler | Dedup race between retry and PendingPoller | `INSERT OR IGNORE` with PRIMARY KEY for atomic dedup |
| Phase 7 | AutoUpdate | In-place EXE replace fails on Windows lock | Use Velopack bootstrapper (update on reboot, not in-flight) |
| Phase 7 | AV false positive | Rust network agent quarantined silently | Sign binary; VirusTotal scan in CI; heartbeat for Noren dashboard |

---

## Sources

- [WritePrinter Win32 docs — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/printdocs/writeprinter)
- [Send raw data to printer (Win32 RAW datatype) — Microsoft Learn](https://learn.microsoft.com/en-us/previous-versions/troubleshoot/windows/win32/win32-raw-data-to-printer)
- [V4 print driver RAW mode 0-byte spool — Microsoft Learn](https://learn.microsoft.com/en-us/troubleshoot/windows/win32/v4-print-driver-raw-mode-pcl-postscript)
- [Getting a USB receipt printer working on Windows — mike42.me](https://mike42.me/blog/2015-04-getting-a-usb-receipt-printer-working-on-windows)
- [Epson ESC/POS GS V cut command reference](https://download4.epson.biz/sec_pubs/pos/reference_en/escpos/gs_cv.html)
- [Pusher Channels WebSocket Protocol — pusher.com](https://pusher.com/docs/channels/library_auth_reference/pusher-websockets-protocol/)
- [Pusher how to authorize users — pusher.com](https://pusher.com/docs/channels/server_api/authorizing-users/)
- [Pusher message size limit 10KB — Bird/Pusher docs](https://docs.bird.com/pusher/channels/channels/limits/what-is-the-message-size-limit-when-publishing-an-event-in-channels)
- [Pusher silent disconnect after 5min — pusher-websocket-swift issue #171](https://github.com/pusher/pusher-websocket-swift/issues/171)
- [Pusher silent disconnect — pusher-websocket-java issue #210](https://github.com/pusher/pusher-websocket-java/issues/210)
- [SmartScreen reputation for Windows developers — Microsoft Learn](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
- [EV certs no longer bypass SmartScreen — ToDesktop blog](https://www.todesktop.com/blog/posts/windows-apps-psa-ev-certs-do-not-grant-immediate-reputation-anymore)
- [SmartScreen OV/EV equivalence — Microsoft Q&A](https://learn.microsoft.com/en-us/answers/questions/417016/reputation-with-ov-certificates-and-are-ev-certifi)
- [self-replace crate — docs.rs](https://docs.rs/self-replace/latest/self_replace/)
- [self_update locked EXE — jaemk/self_update GitHub](https://github.com/jaemk/self_update)
- [HKCU StartupApproved registry key format](https://renenyffenegger.ch/notes/Windows/registry/tree/HKEY_CURRENT_USER/Software/Microsoft/Windows/CurrentVersion/Explorer/StartupApproved/Run/index)
- [CryptProtectData DPAPI — Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)
- [windows-dpapi crate — GitHub sheridans/windows-dpapi](https://github.com/sheridans/windows-dpapi)
- [Rust antivirus false positives — Rust Forum](https://users.rust-lang.org/t/why-my-windows-defender-think-my-rust-file-is-trojan/111832)
- [Tauri Trojan false positive — tauri-apps/tauri issue #2486](https://github.com/tauri-apps/tauri/issues/2486)
- [eframe event loop conflict with tray — emilk/egui issue #2875](https://github.com/emilk/egui/issues/2875)
- [DLE EOT status command — ESC/POS command manual PDF (Aures)](https://aures-support.com/DATA/drivers/Imprimantes/Commande%20ESCPOS.pdf)
- [Velopack Getting Started Rust — docs.velopack.io](https://docs.velopack.io/getting-started/rust)
