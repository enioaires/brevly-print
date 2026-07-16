# Phase 4: Pusher Event Stream - Context

**Gathered:** 2026-07-16
**Status:** Ready for planning
**Note:** Owner delegated all decisions ("pode decidir tudo e já criar o contexto, não entendo
muito sobre a parte técnica") — same posture as Phases 1 and 3. Every decision below is
**locked** (Claude's discretion, exercised). Downstream agents should treat these as decided,
not open.

<domain>
## Phase Boundary

Turn the tray agent into a live event receiver. Build the hand-rolled Pusher WebSocket client,
wire it to the existing `UserEvent::HealthChanged` machine (Phase 3), and deliver `{jobId, type}`
to the print pipeline when events arrive.

Concretely:

1. **Pusher connection** — Connect to `wss://ws-{cluster}.pusher.com/app/{pusher_key}` with
   protocol version 7. Receive `pusher:connection_established`, extract `socket_id`.
2. **Channel auth** — POST to `POST /api/agent/pusher/auth` with `Authorization: Bearer {agentToken}`
   and body `channel=private-tenant-{tenantId}-print&socket_id={socket_id}`. Use the returned
   `auth` string in the subscribe message. Auth must be re-requested on every reconnect using
   the new `socket_id` — cached auth strings are never reused (EVT-02).
3. **Subscription** — Send `{"event":"pusher:subscribe","data":{"channel":"...","auth":"..."}}`;
   await `pusher_internal:subscription_succeeded` → tray transitions `Reconnecting → Connected`
   (green).
4. **Event receipt** — Handle incoming `print-job` events carrying `{"jobId":"…","type":"…"}`.
   Persist the event record immediately (C3 fence); hand off to Phase 5 via `mpsc::Sender`.
5. **Ping/pong health check** — Send `{"event":"pusher:ping","data":"{}"}` every 30 s;
   expect `{"event":"pusher:pong"}` within 30 s. One missed pong = zombie — close and reconnect
   with exponential backoff (C5). Tray transitions `Connected → Reconnecting` on zombie detection.
6. **Reconnect loop** — On disconnect or zombie: wait with exponential backoff, then repeat
   from step 1 (full re-auth, new `socket_id`). Tray stays yellow (`Reconnecting`) until step 3
   completes.

Covers requirements **EVT-01, EVT-02, EVT-03**.

**Out of scope (belongs to other phases):**
- Fetching ESC/POS bytes, WritePrinter, serial write, dedup enforcement, ack — **Phase 5**.
- Printer-failure retry, Windows toast notifications, offline job pull (`pending` endpoint) — **Phase 6**.
- Auto-update flow — **Phase 7**.
- The print pipeline's crash-recovery pass (processing `status='pending'` rows on startup) — **Phase 6** (RES-04). Phase 4 creates the SQLite record; Phase 6 teaches Phase 5 how to reprocess them.
</domain>

<decisions>
## Implementation Decisions

### Pusher credential persistence — D-01
- **D-01:** **Store `pusher_key`, `pusher_cluster`, and `tenant_id` in `ConfigStore` at activation
  time.** The `ActivateResponse` already returns all three (see `noren_client.rs`). Phase 2's save
  step must persist `pusher_key` and `pusher_cluster` to `ConfigStore` in addition to the existing
  fields it already saves (`printer_name`, `printer_type`, `tenant_id`). Phase 4's Pusher module
  reads them from `ConfigStore` at startup. Rejected: compile-time constants — `pusher_key` is the
  Noren app's Pusher app key, which could change (e.g. rotation, staging vs prod env), so it must
  come from the server, not be hard-coded. Rejected: re-fetching from `/api/agent/activate` on every
  startup — unnecessary network call; the values are stable between activations.
- **D-02:** **`agentToken` is already stored in `CredentialStore` (DPAPI) by Phase 2 (ACT-06).**
  Phase 4 reads it from `CredentialStore` to authenticate the Pusher channel auth POST. No change
  to credential storage needed.

### Event handoff to Phase 5 — D-03
- **D-03:** **Hybrid handoff: SQLite insert + `mpsc` channel.** When a `print-job` Pusher event
  arrives:
  1. `INSERT OR IGNORE INTO printed_jobs (job_id, job_type, status, received_at) VALUES (?, ?, 'pending', ?)`
     immediately — the `INSERT OR IGNORE` is the **C3 fence**: it deduplicates at the receive layer,
     so a Pusher re-delivery on reconnect silently no-ops in SQLite and the mpsc send is skipped
     (check `changes() == 0` to detect duplicate). This record survives a crash between receive and
     print, enabling Phase 6 RES-04 reprocessing.
  2. If the insert was not a duplicate: `mpsc::Sender<PrintEvent>.send(event)` for low-latency
     handoff to Phase 5.
  Phase 5 owns `mpsc::Receiver<PrintEvent>`. On normal operation it processes events as they arrive.
  On crash recovery (Phase 6), it also queries `SELECT job_id, job_type FROM printed_jobs WHERE
  status = 'pending'` on startup.
  Rejected: mpsc-only (no SQLite insert by Phase 4) — C3 pitfall: in-memory queue lost on crash,
  and Phase 6 can't detect missed events. Rejected: SQLite-only with polling — higher latency (up
  to 1s polling interval would violate the <1s PRT-06 constraint); mpsc gives immediate wakeup.

### Dev testability — D-04
- **D-04:** **`BREVLY_FAKE_PUSHER_EVENT=<jobId>:<type>` env var injects a synthetic event
  without a real Pusher connection.** When set: skip the WebSocket connection, emit
  `{jobId, type}` after a 1-second delay (simulating the arrival latency), then go idle. This
  lets SC-2 ("agent receives `{jobId, type}` within 500 ms and enqueues for processing") and the
  Phase 4 → Phase 5 handoff path be verified locally before Noren's Pusher auth endpoint ships.
  The real Pusher client is developed and unit-tested against the Pusher protocol spec regardless
  of this env var; the shim only bypasses the network handshake, not the downstream pipeline.
  This is a **dev-only shim** — it MUST NOT appear in release builds. Gate it with
  `#[cfg(debug_assertions)]` OR behind a `debug-tools` Cargo feature (planner to choose); the key
  constraint is that it compiles out in `--release`. Rejected: separate `TestInjector` trait
  abstraction — over-engineering for a single dev-time escape hatch. Rejected: watched file — adds
  cross-platform file-watching complexity; env var is simpler and sufficient.

### Reconnect strategy — D-05
- **D-05:** **Ping interval: 30 s. Zombie threshold: 1 missed pong (30 s timeout after ping
  sent).** No grace period — a single pong miss closes and reconnects. This matches C5's stated
  intent ("ping/pong 30 s obrigatório desde o primeiro dia") and is consistent with the Pusher
  spec's guidance. Strict detection favors reliability over reducing reconnect frequency.
- **D-06:** **Exponential backoff: initial 1 s, doubling each attempt, capped at 60 s. Jitter:
  ±25 % random multiplier.** Series before cap: 1 s, 2 s, 4 s, 8 s, 16 s, 32 s, 60 s, 60 s…
  Jitter prevents thundering herd if multiple agents reconnect simultaneously (restaurant with
  multiple PCs). Jitter does NOT extend the cap beyond 60 s — cap is a ceiling on the jittered
  value.
- **D-07:** **Tray stays `Reconnecting` (yellow) indefinitely during all reconnect attempts.**
  Pusher failures never transition the tray to `Problem` (red) — that state is reserved for printer
  hardware failures (Phase 6, RES-01/RES-02). Sustained network outage = sustained yellow, not red.
  If the agent is offline for hours, it should be yellow throughout and turn green when it reconnects.
  The restaurant owner's mental model: yellow = "internet issue, will recover"; red = "printer issue,
  needs physical attention."

### Pusher protocol implementation scope — D-08
- **D-08:** **Implement the Pusher Channels WebSocket protocol at the minimum required level.**
  Only the messages this agent needs to send/receive:

  | Direction | Event | When |
  |-----------|-------|------|
  | Receive | `pusher:connection_established` | After WS connect; extract `socket_id` from data |
  | Send | `pusher:subscribe` | After auth POST; include `channel` + `auth` |
  | Receive | `pusher_internal:subscription_succeeded` | Channel confirmed; tray → green |
  | Send | `pusher:ping` | Every 30 s |
  | Receive | `pusher:pong` | Expected within 30 s of ping |
  | Receive | `print-job` (app event) | `data` is a JSON-in-JSON string: parse outer JSON first, then parse `data` field as JSON to get `{jobId, type}` |
  | Any | `pusher:error` | Log + treat as disconnect trigger |

  Skip: presence channels, client events, history retrieval, connection state broadcasting.
  The `data` field of app events arrives as a JSON-encoded string (standard Pusher protocol);
  **the parser must double-decode** (outer envelope, then inner data string) — this is a pitfall
  to flag explicitly.

### Claude's Discretion (delegated — planner/executor finalize)
- Exact async task structure: single reconnecting loop task vs. a spawner that creates fresh tasks
  per connect attempt — both work; planner picks based on code clarity.
- Whether to extract a `PusherClient` struct or keep the logic in a module-level function — lean
  toward a struct for testability.
- Error type for the Pusher module — `anyhow::Result` is consistent with the rest of the codebase
  and sufficient for an internal-only client.
- WS URL construction and Pusher cluster routing — standard: `wss://ws-{cluster}.pusher.com/app/{key}?protocol=7&client=brevly-print&version=0.1.0`.
- Auth POST body format — Pusher channel auth uses `application/x-www-form-urlencoded` with
  `channel=...&socket_id=...`; planner to confirm against Pusher spec.
</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase requirements & grading
- `.planning/ROADMAP.md` → **Phase 4: Pusher Event Stream** — Goal + the **4 Success Criteria**
  this phase is graded on (connect+green, receive within 500ms, zombie→reconnect→green, re-auth on
  reconnect).
- `.planning/REQUIREMENTS.md` → **EVT-01, EVT-02, EVT-03** (private channel subscription; re-auth
  per reconnect; zombie detection + exponential backoff reconnect).

### Existing infrastructure (what Phase 4 extends)
- `src/main.rs` — the `UserEvent` enum and `App::user_event()` handler. Phase 4 adds a new
  `UserEvent` variant for incoming print events (or reuses existing if the planner prefers an
  `mpsc::Receiver` polled in `about_to_wait`) AND drives `HealthChanged(Reconnecting)` /
  `HealthChanged(Connected)` from the Pusher task via `EventLoopProxy`. Pitfall C2 applies: all
  tray state changes stay on the event-loop thread; the Pusher async task MUST NOT touch tray
  directly.
- `src/health_state.rs` — `HealthState { Connected, Reconnecting, Problem }` — Phase 4 drives
  `Connected` (after subscription_succeeded) and `Reconnecting` (on zombie / disconnect). `Problem`
  is Phase 6.
- `src/tray_runtime.rs` — `TrayRuntime::apply_health()` — called in `user_event()` when
  `HealthChanged` arrives. Phase 4 drives the first real calls (Phase 3 only seeded `Connected`
  at startup).
- `src/noren_client.rs` — contains `noren_base_url()`, `ActivateResponse` (which includes
  `pusher_key`, `pusher_cluster`, `tenant_id`). Phase 4 adds the Pusher channel auth function
  (`POST /api/agent/pusher/auth`) to this module, consistent with its role as the Noren HTTP client.
- `src/config_store.rs` — `ConfigStore` KV table. Phase 4 depends on Phase 2 having stored
  `pusher_key` and `pusher_cluster` here (D-01). Phase 4 reads them at startup.
- `src/credential_store/` — `CredentialStore::get()` returns `agentToken` for the auth POST
  Bearer header (D-02). No new storage needed.

### Carried decisions, pitfalls & architecture
- `.planning/STATE.md` → "Accumulated Context": locked decisions + pitfalls, especially:
  - **C3**: dedup in-memory lost on crash → SQLite `printed_jobs` with `INSERT OR IGNORE` is the
    only correct fence (directly influences D-03).
  - **C5**: Pusher zombie connection >5 min → ping/pong 30s mandatory (directly influences D-05).
  - Cross-platform build constraint: Pusher client code must compile on Linux (portable core).
    The `tokio-tungstenite` dependency is cross-platform; no `#[cfg(windows)]` guard needed on
    the Pusher module itself.
- `.planning/phases/03-tray-runtime-first-distributable/03-CONTEXT.md` → **D-02** (Phase 4 starts
  in `Reconnecting` until subscription_succeeded → then `Connected`), **D-04** (`HealthChanged`
  via `EventLoopProxy`, pitfall C2), **D-08** (`EventLoopProxy` + `UserEvent` established pattern
  for background→tray communication).

### External protocol specs
- [Pusher Channels WebSocket Protocol](https://pusher.com/docs/channels/library_auth_reference/pusher-websockets-protocol/) —
  event names, envelope format, `socket_id` extraction, `pusher_internal:subscription_succeeded`,
  ping/pong format. **The `data` field in app events is JSON-in-JSON (double-decode required) —
  a critical protocol detail (D-08).**
- [Pusher Private Channel Auth](https://pusher.com/docs/channels/server_api/authenticating-users/) —
  the auth string format (`{app_key}:{HMAC-SHA256(socket_id + ":" + channel)}`) that the Noren
  backend returns and the agent sends in the subscribe message.

### Stack references (from CLAUDE.md)
- `tokio-tungstenite` 0.26.x — async WebSocket client (established in CLAUDE.md as the Pusher
  transport).
- `reqwest` 0.13.x — HTTP client for the Pusher auth POST (already in use for `activate()`).
- `hmac` + `sha2` — HMAC-SHA256 for the HMAC verification notes (server-side; agent just uses the
  auth string returned by Noren, does not compute HMAC itself — but planner should know why the
  Noren endpoint exists).
- `tokio` 1.x — the async runtime that drives the Pusher loop and mpsc channels.
</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/noren_client.rs` — `noren_base_url()` resolves the Noren URL (compile-time env or default).
  Phase 4 adds `pusher_auth()` to this module: `async fn pusher_auth(client, base_url, agent_token,
  channel, socket_id) -> Result<String, PusherAuthError>`. Same pattern as `activate()`.
- `src/main.rs` — `UserEvent` enum already exists with `HealthChanged(HealthState)` (from Phase 3).
  Phase 4 adds a `PrintEventReceived(PrintEvent)` variant (or uses a standalone `mpsc::Receiver`
  polled in `about_to_wait` — planner decides). The `create_proxy()` wiring is already in place.
- `src/config_store.rs` — `ConfigStore::get(key)` / `ConfigStore::set(key, value)` — Phase 4 reads
  `pusher_key` and `pusher_cluster` via these. Schema migration v1 already exists; no new migration
  needed (keys are stored as existing KV pairs, not new columns).
- `src/credential_store/` — the startup credential read already exists; Phase 4 makes the same
  `CredentialStore::get()` call to get `agentToken` for the auth POST header.

### Established Patterns
- **Background → tray via EventLoopProxy**: `event_loop.create_proxy()` → `UserEvent` dispatch →
  `App::user_event()` → `TrayRuntime::apply_health()`. All new Pusher-related state changes (health
  transitions, print event delivery) MUST follow this pattern (C2 risk if violated).
- **`#[cfg(windows)]` gating**: Pusher module itself is cross-platform (tokio-tungstenite runs on
  Linux). Do NOT gate the Pusher module. However, the call from `main.rs` that starts the Pusher
  task occurs inside the Runtime mode block which is already Windows-only — that's sufficient.
  The `src/noren_client.rs` module (no cfg gate) is the right home for `pusher_auth()`.
- **`anyhow::Result` for internal errors**: consistent with `noren_client.rs` and other modules.
- **`option_env!` for compile-time URL**: `noren_base_url()` uses this pattern; replicate for
  any compile-time Pusher config overrides needed for staging environments.

### Integration Points
- `main.rs` Runtime mode → spawn a `tokio::task` for the Pusher reconnect loop, passing an
  `EventLoopProxy<UserEvent>` clone and `mpsc::Sender<PrintEvent>`.
- The `mpsc::Receiver<PrintEvent>` goes to the Phase 5 print worker (also spawned as a tokio task
  in Runtime mode).
- `ConfigStore` read at Runtime mode startup (before spawning the Pusher task) to get
  `pusher_key`, `pusher_cluster`, `tenant_id`.
- `CredentialStore::get()` at Runtime mode startup to get `agentToken` (same call already exists
  at startup for the credential probe — may be re-read or passed through).
- SQLite `INSERT OR IGNORE INTO printed_jobs` on each received event (the C3 dedup fence — D-03).
  The Pusher task needs a `rusqlite::Connection` or a shared `Arc<Mutex<Connection>>`. Planner:
  confirm thread-safe access pattern (the existing code passes `Arc<Mutex<Connection>>` — check
  `config_store.rs` or `main.rs` for the established pattern).
</code_context>

<specifics>
## Specific Ideas

- **Channel name**: `private-tenant-{tenantId}-print` (confirmed in ROADMAP.md — note the `-print`
  suffix, not `-kitchen` which is the old QZ Tray channel in the Noren codebase).
- **WS URL**: `wss://ws-{pusher_cluster}.pusher.com/app/{pusher_key}?protocol=7&client=brevly-print&version=0.1.0`.
- **Auth POST**: `application/x-www-form-urlencoded` body: `channel=private-tenant-{tenantId}-print&socket_id={socket_id}`.
  Authorization header: `Bearer {agentToken}`.
- **Auth response**: Noren returns `{"auth": "{key}:{hmac}"}` — the agent uses the `auth` value
  directly in the subscribe message.
- **Double-decode pitfall**: `print-job` event: outer envelope is `{"event":"print-job","channel":"...","data":"..."}` where `data` is a **JSON-encoded string** (not an object). Parse the outer JSON, then `serde_json::from_str::<PrintEvent>(&data_str)`. Missing this step is a pitfall that silently fails to dispatch events.
- **Fake event env var** (D-04): `BREVLY_FAKE_PUSHER_EVENT=abc123:order` — `jobId` is `abc123`,
  `type` is `order`. Format: `{jobId}:{type}` split on the first colon. Parse at startup; if set
  and `cfg(debug_assertions)`, bypass the WS connection entirely.
- **Blockers**: Phase 4 completion is gated on Noren shipping `POST /api/agent/pusher/auth` +
  event emission. D-04's fake-event shim lets the Phase 4 → Phase 5 handoff pipeline be verified
  locally before that. SC-1 (connect + green) and SC-3 (zombie reconnect) require a real Pusher
  connection but do NOT require Noren's auth endpoint if a test Pusher app is used for integration
  testing (planner may include this as an integration test setup note).
</specifics>

<deferred>
## Deferred Ideas

- **Presence channel / online indicator** — "which agents are online" dashboard feature. Not part
  of Phase 4 (private channel only, no presence). Deferred to a future observability phase if
  owner requests OBS-01.
- **Pusher connection state broadcasting** — reporting connection status back to Noren (not in
  EVT-01..EVT-03). Deferred to a potential OBS-01 phase.
- **Multiple event types in Phase 4** — only `print-job` is handled. Other Pusher event types
  (e.g., config update push) could arrive in future; Phase 4 logs-and-ignores unknown events
  gracefully, leaving the extension point for later.

None of the above expanded Phase 4 scope — discussion stayed within the Pusher event stream
boundary.
</deferred>

---

*Phase: 04-pusher-event-stream*
*Context gathered: 2026-07-16*
