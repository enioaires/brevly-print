# Phase 4: Pusher Event Stream — Research

**Researched:** 2026-07-16
**Domain:** Async WebSocket client (Pusher Channels protocol) + SQLite handoff in Rust/Tokio
**Confidence:** HIGH — all core findings verified against Noren source, tokio-tungstenite
lockfile, and Pusher protocol docs.

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Store `pusher_key`, `pusher_cluster`, `tenant_id` in `ConfigStore` at activation
  time. Phase 4 reads them from `ConfigStore` at startup.
- **D-02:** `agentToken` already in `CredentialStore` (DPAPI) — Phase 4 reads it for the
  Pusher auth POST. No credential storage change needed.
- **D-03:** Hybrid handoff: SQLite `INSERT OR IGNORE INTO printed_jobs` (C3 fence/dedup) +
  `mpsc::Sender<PrintEvent>` for low-latency pass to Phase 5.
- **D-04:** `BREVLY_FAKE_PUSHER_EVENT=<jobId>:<type>` injects a synthetic event in debug
  builds only. Must compile out in `--release`. Gate with `#[cfg(debug_assertions)]` or
  `debug-tools` Cargo feature.
- **D-05:** Ping every 30 s. Zombie = 1 missed pong (30 s timeout after ping). Strict.
- **D-06:** Backoff: 1 s initial, doubles each attempt, 60 s cap, ±25 % jitter. Jitter does
  not extend beyond the cap.
- **D-07:** Tray stays `Reconnecting` (yellow) indefinitely during all reconnect attempts.
  `Problem` (red) is reserved for printer hardware failures (Phase 6).
- **D-08:** Minimal Pusher protocol — only the six message types needed. Double-decode the
  `data` field for app events (`print:job`).

### Claude's Discretion

- Exact async task structure (single reconnecting loop vs. spawner per attempt).
- `PusherClient` struct vs. module-level function — lean toward struct for testability.
- Error type — `anyhow::Result` consistent with codebase.
- WS URL construction per spec.
- Auth POST body format (see research correction below).

### Deferred Ideas (OUT OF SCOPE)

- Presence channel / online indicator (OBS-01, future)
- Pusher connection state broadcasting (OBS-01, future)
- Multiple event types beyond `print:job` — log-and-ignore unknown events
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| EVT-01 | Connect to private Pusher channel and receive `{jobId, type}` print events | tokio-tungstenite `connect_async` + Pusher subscribe flow verified |
| EVT-02 | Re-auth on every reconnect with fresh `socket_id` — never reuse cached auth strings | Verified in Noren auth endpoint: `socket_id` is bound to HMAC; new socket = new auth |
| EVT-03 | Ping/pong zombie detection + exponential backoff reconnect | tokio `select!` pattern for concurrent ping timer + WS reader documented |
</phase_requirements>

---

## Summary

Phase 4 builds a hand-rolled Pusher Channels WebSocket client in Rust. The decisions in
CONTEXT.md are complete and well-scoped. Research confirms all protocol details are standard
and well-supported by the existing stack (`tokio-tungstenite 0.30`, `reqwest 0.13`,
`rusqlite 0.40`). No new Cargo dependencies are required.

**Critical correction from Noren source (authoritative):** The Pusher event name emitted by
Noren is `print:job` (colon), not `print-job` (hyphen) as written in CONTEXT.md D-08 and
the Specifics section. The live Noren code calls
`pusherServer.trigger(..., 'print:job', ...)`. The Rust agent must match this name exactly.
The POST body parameter for the channel is `channel_name` (not `channel`) — confirmed by
both the Pusher JS SDK and the Noren auth endpoint source.

**SQLite access pattern decision needed:** The current `App` struct holds a plain
`rusqlite::Connection` (not `Arc<Mutex<Connection>>`). The Pusher background task needs to
write to SQLite from a different thread. Phase 4 must resolve this before the planner can
assign the SQLite INSERT task. The recommended approach is to open a **second connection**
for the Pusher task (SQLite WAL mode supports concurrent writers from the same process), or
wrap the existing connection in `Arc<Mutex<Connection>>`. Both approaches are documented
below.

**Primary recommendation:** Open a second `rusqlite::Connection` in the Pusher task,
scoped to Runtime mode startup. This avoids retrofitting `Arc<Mutex<>>` throughout the
activation window code, and SQLite handles concurrent access correctly when WAL mode is
enabled.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| WebSocket connection lifecycle | Background async task (tokio) | — | Network I/O must not block the Win32 event loop thread |
| Channel authentication | Background async task (tokio) | Noren backend (auth HMAC) | `reqwest` HTTP POST from the same task that initiates the WS connection |
| Event dedup fence | SQLite `printed_jobs` table | In-memory mpsc | SQLite INSERT is the durable fence; mpsc is the latency path |
| Tray state transitions | winit event loop (via EventLoopProxy) | — | C2 constraint: all tray changes must happen on the event-loop thread |
| Health-state drive (Connected/Reconnecting) | Pusher task → EventLoopProxy → App::user_event | TrayRuntime::apply_health | Indirect path through UserEvent::HealthChanged required |
| Dev event injection (D-04) | `#[cfg(debug_assertions)]` startup branch | — | Compiles out in --release; bypasses WS entirely |

---

## Standard Stack

### Already in Cargo.toml — No New Dependencies

Phase 4 introduces **zero new Cargo dependencies**. All libraries needed are already
declared in `Cargo.toml`:

| Library | Lockfile Version | Purpose | How Used |
|---------|-----------------|---------|----------|
| `tokio-tungstenite` | 0.30.0 | Async WebSocket client | `connect_async()` to Pusher WSS endpoint |
| `futures-util` | 0.3.32 (transitive) | `StreamExt::next()`, `SinkExt::send()` | Stream/Sink traits on `WebSocketStream` |
| `tokio` | 1.x | Async runtime, `select!`, `mpsc`, `time::interval` | Reconnect loop, ping timer, event handoff |
| `reqwest` | 0.13.4 | HTTP client for Pusher auth POST | `POST /api/agent/pusher/auth` with form body |
| `serde_json` | 1.0.150 | Pusher envelope parsing + double-decode | JSON parsing for outer envelope and inner `data` string |
| `rusqlite` | 0.40.1 | SQLite INSERT OR IGNORE for C3 dedup fence | `printed_jobs` table insert on event receipt |

**`futures-util` requires explicit `Cargo.toml` declaration.** It is a transitive dependency
(brought by `tokio-tungstenite`) but must be added to `[dependencies]` to use
`StreamExt`/`SinkExt` traits directly in Phase 4 code without relying on re-export
stability. [VERIFIED: tokio-tungstenite-0.30.0/src/lib.rs uses `futures_util::sink::SinkExt`
and `futures_util::stream::Stream` internally but does not re-export them as public items.]

**Installation (the only change to Cargo.toml):**
```bash
# Add futures-util explicitly so StreamExt/SinkExt are referenceable in src/
# The version must match the one already resolved in Cargo.lock (0.3.32)
cargo add futures-util@0.3
```

---

## Package Legitimacy Audit

Phase 4 adds one explicit dependency (`futures-util`); the others were already present.

| Package | Registry | slopcheck | Notes | Disposition |
|---------|----------|-----------|-------|-------------|
| `tokio-tungstenite` | crates.io | [OK] | Already in Cargo.toml; 0.30.0 in lockfile | Approved |
| `futures-util` | crates.io | [OK] | Transitive dep, needs explicit declaration | Approved |
| `hmac` | crates.io | [OK] | Already in Cargo.toml (Phase 4 does not use it client-side) | Approved |
| `sha2` | crates.io | [OK] | Already in Cargo.toml (Phase 4 does not use it client-side) | Approved |
| `serde_json` | crates.io | [OK] | Already in Cargo.toml | Approved |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

*slopcheck 0.6.1 ran cleanly on all packages — all [OK].*
[VERIFIED: slopcheck 0.6.1 output confirmed OK for all four packages]

---

## Architecture Patterns

### System Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│  Runtime mode startup (main.rs)                                  │
│                                                                  │
│  ConfigStore::get(pusher_key, pusher_cluster, tenant_id)        │
│  CredentialStore::load() → agentToken                           │
│  Connection::open(db_path) → pusher_conn  (2nd SQLite conn)     │
│  mpsc::channel::<PrintEvent>() → (tx, rx)                       │
│                                                                  │
│  tokio::spawn ──► [Pusher reconnect loop task]                  │
│  tokio::spawn ──► [Phase 5 print worker] ◄── rx                │
└─────────────────────────────────────────────────────────────────┘
           │ EventLoopProxy<UserEvent>
           │ mpsc::Sender<PrintEvent>
           │ Arc (pusher_conn / 2nd conn)
           ▼
┌─────────────────────────────────────────────────────────────────┐
│  Pusher reconnect loop (tokio task)                              │
│                                                                  │
│  loop {                                                          │
│    proxy.send(HealthChanged(Reconnecting))                       │
│                                                                  │
│    1. connect_async(ws_url) ────────────────► Pusher WSS        │
│    2. recv pusher:connection_established                         │
│       extract socket_id                                          │
│    3. reqwest POST /api/agent/pusher/auth                       │
│       body: channel_name=...&socket_id=...                      │
│       → { "auth": "key:hmac" }                                   │
│    4. send pusher:subscribe {channel, auth}                      │
│    5. recv pusher_internal:subscription_succeeded                │
│       proxy.send(HealthChanged(Connected))                       │
│                                                                  │
│    loop {                                                        │
│      select! {                                                   │
│        msg = ws.next()   ──► dispatch message                   │
│        _ = ping_interval ──► send pusher:ping                   │
│                               arm pong_deadline                 │
│        _ = pong_deadline ──► zombie! break inner loop           │
│      }                                                           │
│                                                                  │
│      if msg == print:job {                                       │
│        INSERT OR IGNORE INTO printed_jobs                        │
│        if inserted: tx.send(PrintEvent)                          │
│      }                                                           │
│    }                                                             │
│                                                                  │
│    proxy.send(HealthChanged(Reconnecting))                       │
│    tokio::time::sleep(backoff)  // jittered exponential         │
│  }                                                               │
└─────────────────────────────────────────────────────────────────┘
```

### Recommended Project Structure

```
src/
├── pusher/
│   ├── mod.rs         # pub use PusherClient, PusherConfig, run_pusher_loop
│   ├── client.rs      # PusherClient struct: connect, auth, subscribe, event loop
│   ├── protocol.rs    # PusherEnvelope, PrintEvent, parse_envelope(), double_decode()
│   └── backoff.rs     # ExponentialBackoff: next_delay(), jitter
├── main.rs            # Runtime mode: spawn pusher task + print worker
├── noren_client.rs    # Add pusher_auth() here (consistent with existing pattern)
└── ... (existing files unchanged)
```

The `pusher` module is cross-platform (no `#[cfg(windows)]` needed — `tokio-tungstenite`
compiles on Linux). Only the spawn site in `main.rs` Runtime mode is in a Windows context.

### Pattern 1: Pusher Connection and Subscribe Flow

```rust
// Source: Pusher Channels WebSocket Protocol spec + tokio-tungstenite 0.30 docs
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{StreamExt, SinkExt};

pub async fn connect_and_subscribe(
    config: &PusherConfig,
    agent_token: &str,
    http: &reqwest::Client,
) -> anyhow::Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, String)> {
    // Step 1: Connect
    let ws_url = format!(
        "wss://ws-{}.pusher.com/app/{}?protocol=7&client=brevly-print&version=0.1.0",
        config.cluster, config.key
    );
    let (mut ws, _) = connect_async(&ws_url).await
        .context("WebSocket connect failed")?;

    // Step 2: Receive pusher:connection_established, extract socket_id
    let socket_id = recv_connection_established(&mut ws).await?;

    // Step 3: Auth POST (channel_name, not channel — verified from Noren source)
    let channel = format!("private-tenant-{}-print", config.tenant_id);
    let auth = pusher_auth(http, &config.auth_url, agent_token, &channel, &socket_id).await?;

    // Step 4: Subscribe
    let subscribe_msg = serde_json::json!({
        "event": "pusher:subscribe",
        "data": { "channel": channel, "auth": auth }
    });
    ws.send(Message::Text(subscribe_msg.to_string().into())).await?;

    // Step 5: Await subscription_succeeded (handled in caller's event loop)
    Ok((ws, channel))
}
```

### Pattern 2: Event Loop with Ping/Pong (tokio::select!)

```rust
// Source: Pusher Channels protocol spec, tokio docs
use tokio::time::{interval, timeout, Duration};

let mut ping_timer = interval(Duration::from_secs(30));
let mut awaiting_pong = false;
let pong_timeout = Duration::from_secs(30);

loop {
    tokio::select! {
        msg = ws.next() => {
            match msg {
                Some(Ok(Message::Text(text))) => {
                    let env = parse_envelope(&text)?;
                    match env.event.as_str() {
                        "pusher:pong" => { awaiting_pong = false; }
                        "pusher_internal:subscription_succeeded" => {
                            proxy.send_event(UserEvent::HealthChanged(HealthState::Connected)).ok();
                        }
                        "print:job" => {  // NOTE: colon, not hyphen
                            handle_print_job(&env, &mut conn, &tx).await?;
                        }
                        "pusher:error" => {
                            // Log and treat as disconnect (error codes 4000-4099 = no reconnect,
                            // 4100-4199 = backoff reconnect, 4200-4299 = immediate reconnect)
                            break;
                        }
                        _ => { /* log unknown events, continue */ }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(e)) => { eprintln!("WS error: {e}"); break; }
                _ => {} // Ping/Pong frames at protocol level — tungstenite handles auto-pong
            }
        }
        _ = ping_timer.tick() => {
            if awaiting_pong {
                // Zombie: missed pong
                break;
            }
            let ping = serde_json::json!({"event":"pusher:ping","data":{}});
            ws.send(Message::Text(ping.to_string().into())).await.ok();
            awaiting_pong = true;
            // Reset: next tick is 30s later — pong_deadline is implicit in awaiting_pong + tick
        }
    }
}
```

**Ping/pong implementation note:** `D-05` calls for a 30 s timeout after each ping. The
above pattern uses the interval itself as the pong deadline: `awaiting_pong` is set `true`
on ping; on the next 30 s tick if `awaiting_pong` is still `true`, the connection is
declared zombie. This is correct — the effective pong window is 30 s (one tick). No
separate `timeout()` wrapper needed.

### Pattern 3: Double-Decode for `print:job` Event Data

```rust
// Source: Pusher Channels WebSocket Protocol spec — "data field is a JSON-encoded string"
#[derive(Deserialize)]
struct PusherEnvelope {
    event: String,
    data: serde_json::Value,  // may be String or Object
    #[serde(default)]
    channel: String,
}

#[derive(Deserialize)]
struct PrintJobPayload {
    #[serde(rename = "jobId")]
    job_id: String,
    #[serde(rename = "type")]
    job_type: String,
}

fn parse_print_job(env: &PusherEnvelope) -> anyhow::Result<PrintJobPayload> {
    // Outer envelope is already parsed. Now inner decode:
    // The `data` field for app events is a JSON-encoded STRING, not an object.
    let data_str = match &env.data {
        serde_json::Value::String(s) => s.as_str(),
        // Defensive: if somehow already decoded as object, re-serialize and proceed
        other => return Err(anyhow::anyhow!("Expected JSON string in data, got: {other}")),
    };
    serde_json::from_str::<PrintJobPayload>(data_str)
        .context("Failed to decode print job payload (double-decode)")
}
```

### Pattern 4: Exponential Backoff with Jitter

```rust
// Source: D-06 in CONTEXT.md
fn backoff_delay(attempt: u32) -> Duration {
    let base_ms: u64 = 1000u64.saturating_mul(1u64 << attempt.min(6)); // cap at 64s base
    let capped_ms = base_ms.min(60_000);
    // ±25% jitter: multiply by random in [0.75, 1.25]
    let jitter_factor = 0.75 + rand::random::<f64>() * 0.50;
    let jittered_ms = (capped_ms as f64 * jitter_factor) as u64;
    Duration::from_millis(jittered_ms.min(60_000)) // jitter never exceeds cap
}
```

**Note:** `rand` is not in `Cargo.toml`. Use `tokio::time::sleep` with a simpler jitter
derived from `std::time::SystemTime::now().duration_since(UNIX_EPOCH)` modulo a range, or
add `rand` as a dependency. The planner should choose — either approach is correct.
[ASSUMED: `rand` crate would be the cleaner choice — not yet in Cargo.toml]

### Pattern 5: SQLite INSERT OR IGNORE (C3 Dedup Fence)

```rust
// Source: D-03 in CONTEXT.md + rusqlite docs
fn insert_print_job(
    conn: &rusqlite::Connection,
    job_id: &str,
    job_type: &str,
) -> rusqlite::Result<bool> {
    let changes = conn.execute(
        "INSERT OR IGNORE INTO printed_jobs (job_id, job_type, status, received_at)
         VALUES (?1, ?2, 'pending', datetime('now'))",
        rusqlite::params![job_id, job_type],
    )?;
    Ok(changes > 0) // true = inserted (new), false = duplicate (ignored)
}
```

If `changes == 0`, the event is a Pusher re-delivery or reconnect duplicate — skip the
`mpsc::Sender::send()` call.

### Pattern 6: Pusher Auth POST to Noren

```rust
// Source: Noren source /api/agent/pusher/auth/+server.ts (verified)
// CRITICAL: parameter is "channel_name" (not "channel") — Noren reads body.get('channel_name')
pub async fn pusher_auth(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    channel: &str,
    socket_id: &str,
) -> anyhow::Result<String> {
    let url = format!("{base_url}/api/agent/pusher/auth");
    let params = [("channel_name", channel), ("socket_id", socket_id)];

    let resp = client
        .post(&url)
        .bearer_auth(agent_token)
        .form(&params)
        .send()
        .await
        .context("Pusher auth POST failed")?;

    if resp.status() == 403 {
        anyhow::bail!("Pusher auth 403 — token invalid or channel mismatch");
    }

    #[derive(Deserialize)]
    struct AuthResponse { auth: String }
    let body: AuthResponse = resp.json().await.context("Pusher auth response parse")?;
    Ok(body.auth)
}
```

### Anti-Patterns to Avoid

- **Reusing the auth string across reconnects:** The HMAC is bound to `socket_id`. A new
  WebSocket connection generates a new `socket_id` — the old auth will cause Pusher to
  reject the subscribe message with a 4009 error.
- **Sending `channel` (not `channel_name`) in the auth POST:** Noren reads
  `body.get('channel_name')` — sending `channel` causes a 403.
- **Single-decoding `print:job` data:** The `data` field is a JSON-encoded string. Parsing
  the outer envelope without a second `serde_json::from_str` on the `data` field will
  leave you with a raw string where you expected a struct.
- **Using `print-job` as the event name:** The real event name is `print:job` (colon).
  A hyphen would never match any event Noren emits.
- **Touching tray state from the Pusher task directly:** The Pusher task runs on a tokio
  thread pool. All tray state changes must go through `EventLoopProxy::send_event(UserEvent::HealthChanged(...))`.
- **Calling `awaiting_pong` reset on every pong frame:** Only reset it when the pong is for
  the most recent ping. (In practice, Pusher only echoes one pong per ping, but defensive
  coding requires the flag to be cleared before the timer re-arms.)

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TLS WebSocket connection | Custom TLS setup | `connect_async` with `rustls-tls-webpki-roots` feature | Already configured in Cargo.toml; handles cert validation automatically |
| URL encoding of form body | String concatenation | `reqwest::Client::form(&params)` | Handles percent-encoding; `reqwest` already in use |
| JSON parsing of Pusher envelope | Manual string splitting | `serde_json::from_str` (twice — double-decode) | Handles edge cases in JSON encoding; serde already in use |
| Cross-platform async I/O | Blocking I/O on a thread | `tokio::select!` with `ws.next()` | Non-blocking; `tokio` already the runtime |
| HMAC-SHA256 (for Pusher auth) | Custom HMAC | Not needed — Noren backend does the signing | Agent only uses the auth string returned by Noren; `hmac`/`sha2` in Cargo.toml but not consumed here |

**Key insight:** The agent is a Pusher consumer, not a Pusher SDK reimplementation. It only
needs to handle 6 message types. The ~200 LoC scope (from CONTEXT.md) is correct.

---

## Common Pitfalls

### Pitfall 1: Wrong Pusher Event Name (`print-job` vs `print:job`)

**What goes wrong:** The Rust match arm uses `"print-job"` — the event string from the
CONTEXT.md. The Noren backend emits `"print:job"` (colon). The arm never matches; all print
events are silently swallowed by the `_ => {}` catch-all.

**Why it happens:** CONTEXT.md D-08 and the Specifics section document the event as
`print-job`. The Noren source (agent-print-jobs.ts:50) emits `'print:job'`.

**How to avoid:** Match on `"print:job"` in the event dispatch switch.
[VERIFIED: Noren `agent-print-jobs.ts` line 50: `pusherServer.trigger(..., 'print:job', ...)`]

**Warning signs:** `subscription_succeeded` arrives (tray turns green) but no print events
are ever processed, even when Noren triggers them.

### Pitfall 2: Wrong Auth POST Body Parameter (`channel` vs `channel_name`)

**What goes wrong:** The agent sends `channel=private-tenant-...-print&socket_id=...`. Noren
reads `body.get('channel_name')` — gets `null` — returns 403.

**Why it happens:** The CONTEXT.md Specifics section writes the body as
`channel=...&socket_id=...`. The Pusher JS SDK and Noren source both use `channel_name`.

**How to avoid:** Use `channel_name` as the form field name in the auth POST.
[VERIFIED: Noren `/api/agent/pusher/auth/+server.ts` line 40: `body.get('channel_name')`]
[VERIFIED: Pusher JS SDK channel_authorizer.ts: `"&channel_name=" + encodeURIComponent(...)`]

**Warning signs:** `pusher_auth()` always returns 403 even with a valid token.

### Pitfall 3: Double-Decode Omission

**What goes wrong:** Code parses the outer `PusherEnvelope` but treats `env.data` as the
final object. `env.data` for `print:job` is a `serde_json::Value::String`, not a
`serde_json::Value::Object`. Deserializing into `PrintJobPayload` fails or produces
unexpected output.

**Why it happens:** Pusher protocol wraps the `data` field as a JSON-encoded string for all
app events (not just some). This is counterintuitive — the `data` field looks like
`"data": "{\"jobId\":\"abc\",\"type\":\"order\"}"` (a string, not an object).

**How to avoid:** After parsing the outer envelope, call `serde_json::from_str::<PrintJobPayload>(data_str)` on the extracted string value.
[CITED: https://pusher.com/docs/channels/library_auth_reference/pusher-websockets-protocol/]

### Pitfall 4: Pusher Task Touching Tray Directly (C2)

**What goes wrong:** Code in the Pusher async task calls tray icon methods directly. This
crashes or panics because `tray-icon` requires all tray calls on the Win32 event loop
thread (the thread running `event_loop.run_app()`).

**Why it happens:** The Pusher task runs on tokio's multi-thread pool — a different OS
thread.

**How to avoid:** All health state changes go through
`proxy.send_event(UserEvent::HealthChanged(state))`. The `user_event()` handler in `App`
calls `tray_runtime.apply_health()` — already established pattern in Phase 3.

### Pitfall 5: Single SQLite Connection Concurrency

**What goes wrong:** The Pusher background task tries to write to the same `rusqlite::Connection`
that `App` holds (passed by reference). `rusqlite::Connection` is not `Send` — the compiler
rejects it. Or if worked around unsafely, SQLite returns `SQLITE_BUSY` errors.

**Why it happens:** `App.conn` is a plain `rusqlite::Connection` with no synchronization.
The Pusher task runs on a different thread.

**How to avoid:** Open a **second** `rusqlite::Connection` for the Pusher task in Runtime
mode startup (before spawning the task). Enable WAL mode on both connections:
```rust
conn.pragma_update(None, "journal_mode", "WAL")?;
```
SQLite WAL allows multiple concurrent writers from the same process safely.

Alternatively, wrap `App.conn` in `Arc<Mutex<rusqlite::Connection>>` and pass an `Arc`
clone to the Pusher task — but this requires changing the type throughout `App`, including
in `draw()` and `config_store` call sites. The second-connection approach is simpler.

### Pitfall 6: `futures-util` Not in `Cargo.toml`

**What goes wrong:** Code uses `use futures_util::StreamExt;` which compiles only because
`futures-util` is an indirect dep. Future version bumps may change the transitive dep graph
and break the code without a lockfile pin.

**How to avoid:** Add `futures-util = "0.3"` to `[dependencies]` explicitly.

### Pitfall 7: Auth String Cached Across Reconnects (EVT-02)

**What goes wrong:** The Pusher task stores the `auth` string from the first successful
auth POST, then reuses it after a disconnect. The new `socket_id` from the reconnected
session is different — the old auth signature is invalid for the new socket.

**How to avoid:** Never store the auth string. The subscribe flow always runs in sequence:
connect → extract `socket_id` → POST auth → send subscribe → await subscription_succeeded.
Every reconnect starts from step 1.

### Pitfall 8: `print:job` Event Received Before `subscription_succeeded`

**What goes wrong:** Events are dispatched based on a boolean flag `subscribed`. If the
flag is not set before entering the event dispatch loop (or if `subscription_succeeded` is
missed), `print:job` events are ignored.

**How to avoid:** The inner event loop should be entered only after `subscription_succeeded`
is received. A clean state machine: `connecting → subscribing → subscribed (event loop)`.

---

## Code Examples

### Pusher Envelope Types

```rust
// Source: Pusher Channels WebSocket Protocol spec
// [CITED: https://pusher.com/docs/channels/library_auth_reference/pusher-websockets-protocol/]

#[derive(serde::Deserialize, Debug)]
pub struct PusherEnvelope {
    pub event: String,
    pub data: serde_json::Value,  // String for system events + app events (double-encoded)
    #[serde(default)]
    pub channel: Option<String>,
}

/// Payload inside a `print:job` event's `data` field (after double-decode).
#[derive(serde::Deserialize, Debug, Clone)]
pub struct PrintEvent {
    #[serde(rename = "jobId")]
    pub job_id: String,
    #[serde(rename = "type")]
    pub job_type: String,
}
```

### Connection Established Parsing

```rust
// Source: Pusher Channels WebSocket Protocol spec
// The `data` field of connection_established is also JSON-in-JSON (double-encoded).
// {"event":"pusher:connection_established","data":"{\"socket_id\":\"123.456\",\"activity_timeout\":120}"}

#[derive(serde::Deserialize)]
struct ConnectionEstablishedData {
    socket_id: String,
    activity_timeout: Option<u64>,
}

fn extract_socket_id(env: &PusherEnvelope) -> anyhow::Result<String> {
    let data_str = env.data.as_str()
        .ok_or_else(|| anyhow::anyhow!("connection_established data is not a string"))?;
    let data: ConnectionEstablishedData = serde_json::from_str(data_str)?;
    Ok(data.socket_id)
}
```

Note: `activity_timeout` (default 120 s) is the server-suggested keep-alive interval. Phase
4 uses 30 s (D-05), which is stricter than the server suggestion — that's intentional.

### `pusher_auth` Function (add to `noren_client.rs`)

```rust
// Source: Noren /api/agent/pusher/auth/+server.ts (verified)
// POST body: channel_name=<channel>&socket_id=<socket_id>
// Response: {"auth":"<key>:<hmac>"}
pub async fn pusher_auth(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    channel: &str,
    socket_id: &str,
) -> anyhow::Result<String> {
    #[derive(serde::Deserialize)]
    struct PusherAuthResponse { auth: String }

    let url = format!("{base_url}/api/agent/pusher/auth");
    let resp = client
        .post(&url)
        .bearer_auth(agent_token)
        .form(&[("channel_name", channel), ("socket_id", socket_id)])
        .send()
        .await
        .context("pusher_auth: HTTP transport error")?;

    match resp.status().as_u16() {
        200 => {
            let body: PusherAuthResponse = resp.json().await
                .context("pusher_auth: response parse error")?;
            Ok(body.auth)
        }
        403 => anyhow::bail!("pusher_auth: 403 — invalid token or channel mismatch"),
        status => anyhow::bail!("pusher_auth: unexpected status {status}"),
    }
}
```

### Dev Shim (D-04)

```rust
// Source: CONTEXT.md D-04
// Gate: #[cfg(debug_assertions)] — compiles out in --release
#[cfg(debug_assertions)]
fn try_fake_pusher_event(tx: &tokio::sync::mpsc::Sender<PrintEvent>) -> bool {
    let Some(raw) = std::env::var("BREVLY_FAKE_PUSHER_EVENT").ok() else { return false };
    // Format: jobId:type — split on first colon only
    let (job_id, job_type) = match raw.split_once(':') {
        Some((id, t)) => (id.to_string(), t.to_string()),
        None => {
            eprintln!("[brevly-print] BREVLY_FAKE_PUSHER_EVENT: invalid format (expected jobId:type)");
            return false;
        }
    };
    let tx = tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let _ = tx.send(PrintEvent { job_id, job_type }).await;
    });
    true // caller skips real WebSocket connection
}
```

---

## State of the Art

| Old Approach | Current Approach | Notes | Impact |
|--------------|------------------|-------|--------|
| `pusher-rs` / `pusher` crates | Hand-rolled over `tokio-tungstenite` | Both existing crates are abandoned/unsupported | No change needed — hand-rolled is the right call |
| `tao` event loop | `winit 0.30` | Already established in Phase 1 | Pusher module itself is event-loop-agnostic |
| `native-tls` | `rustls` (webpki-roots) | `tokio-tungstenite` configured with `rustls-tls-webpki-roots` in Cargo.toml | WSS connections use rustls automatically via `connect_async` |

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `rand` crate is the cleanest way to add jitter to backoff | Pattern 4 (backoff) | Low — `std::time` modulo arithmetic is a viable alternative; planner should decide |
| A2 | WAL mode is not yet enabled on the SQLite DB (no pragma in `open_and_migrate`) | Pitfall 5 | If WAL is already enabled elsewhere, no action needed; if not, enabling it is safe and recommended |

**All critical claims (event name, POST body parameters, auth response format) are
[VERIFIED] from Noren source or [CITED] from Pusher protocol docs.**

---

## Open Questions

1. **SQLite concurrency strategy**
   - What we know: `App.conn` is a plain `rusqlite::Connection`; the Pusher task needs SQLite write access.
   - What's unclear: Which approach the planner prefers — second connection (recommended here) or `Arc<Mutex<Connection>>`.
   - Recommendation: Open a second connection for the Pusher task. Enable WAL mode pragma on both connections at open time. This is the simplest path that avoids refactoring `App`.

2. **Jitter source for backoff**
   - What we know: D-06 specifies ±25 % jitter.
   - What's unclear: Whether to add `rand` to `Cargo.toml` or use `std::time` modulo arithmetic.
   - Recommendation: Add `rand = "0.8"` (one dep); cleaner than bit-twiddling with `SystemTime`. Planner decides.

3. **`print:job` vs D-04 fake event format**
   - What we know: The real Pusher event is `print:job`. D-04 parses `BREVLY_FAKE_PUSHER_EVENT` as `{jobId}:{type}` split on the first colon.
   - What's unclear: If `job_type` contains a colon (unlikely for `order`/`dispatch`/`closing`/`cancel`), the split is fine. No risk in practice.
   - Recommendation: Use `split_once(':')` — safe for all defined types.

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `tokio-tungstenite` | Pusher WebSocket | ✓ (in Cargo.lock) | 0.30.0 | — |
| `futures-util` | StreamExt / SinkExt traits | ✓ (transitive) | 0.3.32 | — (must be explicit dep) |
| `reqwest` | Pusher auth POST | ✓ | 0.13.4 | — |
| `rusqlite` | C3 dedup INSERT | ✓ | 0.40.1 | — |
| Noren `POST /api/agent/pusher/auth` | Channel auth (EVT-02) | Available in Noren source | Implemented + tested | D-04 fake shim for dev |
| Pusher Channels service | WebSocket endpoint | ✓ (cloud, no local setup) | — | D-04 fake shim for dev |

**Missing dependencies with no fallback:** None.

**Missing dependencies with fallback:**
- Noren auth endpoint not yet deployed to production → D-04 fake event shim lets Phase 4 → Phase 5 pipeline be verified locally.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) |
| Config file | none (uses default Cargo test harness) |
| Quick run command | `cargo test --lib` |
| Full suite command | `cargo test` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| EVT-01 | `pusher_auth()` maps 200/403/transport to typed results | unit (integration-style) | `cargo test --test pusher_auth_test` | ❌ Wave 0 |
| EVT-01 | `parse_print_job()` double-decodes `data` field correctly | unit | `cargo test --lib -- pusher::protocol::tests` | ❌ Wave 0 |
| EVT-01 | `extract_socket_id()` parses `connection_established` correctly | unit | `cargo test --lib -- pusher::protocol::tests` | ❌ Wave 0 |
| EVT-02 | Auth string not cached — fresh auth POST on each reconnect | unit (mock HTTP stub) | `cargo test --test pusher_auth_test` | ❌ Wave 0 |
| EVT-03 | `backoff_delay(attempt)` never exceeds 60 s cap | unit | `cargo test --lib -- pusher::backoff::tests` | ❌ Wave 0 |
| EVT-03 | Zombie detection: `awaiting_pong = true` on second ping tick triggers break | unit (via mock ws) | `cargo test --lib -- pusher::client::tests` | ❌ Wave 0 |
| D-03 | `insert_print_job()` INSERT OR IGNORE returns `false` on duplicate | unit | `cargo test --lib -- pusher::client::tests` | ❌ Wave 0 |
| D-04 | `BREVLY_FAKE_PUSHER_EVENT` shim parses `jobId:type` correctly | unit | `cargo test --lib -- pusher::tests` | ❌ Wave 0 |

Tests follow the established pattern from `tests/noren_client_test.rs`:
- Mock TCP listener (for `pusher_auth` HTTP tests)
- In-process unit tests for pure functions (`protocol.rs`, `backoff.rs`)
- `#[cfg(debug_assertions)]` guard on the fake-event shim test

### Sampling Rate

- **Per task commit:** `cargo test --lib`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green + manual smoke (SC-1 tray turns green, SC-2 fake event processed)

### Wave 0 Gaps

- [ ] `tests/pusher_auth_test.rs` — covers EVT-01, EVT-02 HTTP contract
- [ ] `src/pusher/protocol.rs` inline tests — double-decode, socket_id extraction
- [ ] `src/pusher/backoff.rs` inline tests — cap, jitter range
- [ ] `src/pusher/client.rs` inline tests — INSERT OR IGNORE dedup logic

---

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | yes (token auth) | Bearer `agentToken` on every Pusher auth POST — from `CredentialStore` (DPAPI), never hardcoded |
| V3 Session Management | yes (WS session) | `socket_id` never persisted; new auth on every reconnect (EVT-02) |
| V4 Access Control | yes (channel isolation) | Noren hard 403 on wrong channel (T-38-05); agent only subscribes to its tenant's channel |
| V5 Input Validation | yes | `serde_json` with typed structs for Pusher envelope; `job_id` / `job_type` passed as SQL parameters only |
| V6 Cryptography | n/a (agent side) | HMAC computed by Noren server; agent only uses the returned auth string — never computes HMAC |

### Known Threat Patterns for this Stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Cross-tenant channel subscription | Elevation of Privilege | Noren auth endpoint hard-403 if `channel_name` ≠ `private-tenant-{tenantId}-print` (T-38-05, verified in server source) |
| `agentToken` log exposure | Information Disclosure | `pusher_auth()` must not log the Bearer token — consistent with `activate()` (T-02-02) |
| Zombie connection event loss | Denial of Service | D-05 ping/pong detection with 30 s timeout; indefinite reconnect loop (D-06/D-07) |
| Pusher re-delivery printing duplicate | Tampering | C3 fence: `INSERT OR IGNORE` dedup before `mpsc::send` — verified correct in D-03 |
| Auth string reuse after reconnect | Spoofing | EVT-02: auth string never cached; fresh POST on every reconnect |

---

## Sources

### Primary (HIGH confidence — verified from authoritative sources)

- Noren source `/src/routes/api/agent/pusher/auth/+server.ts` — auth endpoint POST body uses `channel_name`, Bearer header; returns `{"auth": "..."}` [VERIFIED]
- Noren source `/src/lib/server/agent-print-jobs.ts` — event name is `'print:job'` (colon), not `'print-job'` [VERIFIED]
- `tokio-tungstenite 0.30.0` lockfile entry (`Cargo.lock`) — version confirmed [VERIFIED]
- `tokio-tungstenite 0.30.0` source (`lib.rs`) — `connect_async`, `WebSocketStream`, `StreamExt`/`SinkExt` via `futures-util` [VERIFIED: registry source at `~/.cargo/registry/src/`]
- Pusher JS SDK `channel_authorizer.ts` — `channel_name` param name confirmed [CITED: github.com/pusher/pusher-js]

### Secondary (MEDIUM confidence — official docs)

- Pusher Channels WebSocket Protocol — event envelope format, double-encoded `data` field, connection_established, subscribe, subscription_succeeded, ping/pong format, error code ranges [CITED: https://pusher.com/docs/channels/library_auth_reference/pusher-websockets-protocol/]
- Pusher auth signature spec — `socket_id:channel_name` HMAC string, `key:hmac` auth format [CITED: https://pusher.com/docs/channels/library_auth_reference/auth-signatures/]

### Tertiary (LOW confidence — not needed)

None — all critical claims were verified from authoritative sources.

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all packages in Cargo.lock; no new deps beyond `futures-util` explicit declaration
- Protocol details: HIGH — verified from Noren source and Pusher official spec
- Architecture patterns: HIGH — extend existing Phase 3 `EventLoopProxy` pattern; SQLite pattern matches `config_store.rs`
- Pitfalls: HIGH — event name and POST body parameter mismatches verified from Noren source directly

**Research date:** 2026-07-16
**Valid until:** 2026-08-16 (stable protocol specs; Noren source is in the same repo)
