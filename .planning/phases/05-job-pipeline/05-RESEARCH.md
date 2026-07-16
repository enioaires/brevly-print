# Phase 5: Job Pipeline - Research

**Researched:** 2026-07-16
**Domain:** Rust async print worker — mpsc consumer, HTTP fetch, base64 decode, SQLite update, ESC/POS dispatch
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**D-01:** Create `src/print_worker.rs` with `pub async fn run_print_worker(rx, agent_token, base_url, db_path, http)` — analogous to `src/pusher/client.rs` / `run_pusher_loop()`. Spawned in `main.rs` Runtime block immediately after the Pusher task spawn. `print_rx` goes directly to the worker task; it is NOT stored in `App` any longer.

**D-02:** Remove `App._print_rx: Option<Receiver<PrintEvent>>` from the `App` struct. That field was a Phase 4 placeholder. Phase 5 consumes the receiver at spawn time; `App` no longer needs to hold it. `lib.rs` gets `pub mod print_worker`.

**D-03:** The print worker opens its own `rusqlite::Connection` with `PRAGMA journal_mode=WAL` (same pattern as the Pusher task). At startup it reads `enabled_types`, `printer_name`, `printer_type` from ConfigStore. Constructs `PrinterId` and calls `printer_from_entry()` to hold `Box<dyn Printer>` for the task lifetime. If any read fails: log fatal and return. If `enabled_types` is missing or empty: default to allow-all (fail-safe).

**D-04:** `noren_client.rs` gets two new async functions (`anyhow::Result` pattern):
- `fetch_job_bytes(client, base_url, agent_token, job_id) -> anyhow::Result<Vec<u8>>` — `GET /api/agent/jobs/{jobId}/bytes`, response `{"bytes": "<base64>"}`, returns decoded bytes.
- `ack_job(client, base_url, agent_token, job_id) -> anyhow::Result<()>` — `POST /api/agent/jobs/{jobId}/ack`, 200 → `Ok(())`, 409 → `Ok(())` (idempotent), others → `Err`.
`agent_token` passed via `.bearer_auth()` — never logged (T-02-02).

**D-05:** Add `base64 = "0.22"` to `[dependencies]` in `Cargo.toml`. API: `use base64::Engine as _;` + `base64::engine::general_purpose::STANDARD.decode(s)`.

**D-06:** Job type strings: `"order"` (PRT-02), `"dispatch"` (PRT-03), `"closing"` (PRT-04), `"cancel"` (bonus). **MUST VERIFY** exact strings against Noren event emission code before Phase 5 ships. Print worker does not route by type — all types use same fetch→print path. Type is only used for the `enabled_types` filter.

**D-07 (enabled_types filter, PRT-09):** If `event.job_type` is in `enabled_types` (or `enabled_types` is empty/missing): proceed. If NOT in `enabled_types`: mark `status='printed'` + send ack + skip print. No error raised.

**D-08 (SQLite status progression):**
- `pending` → `printed`: successful `print_raw()` call OR disabled job type (D-07)
- `pending` → *(unchanged)*: fetch or print failure — logged, left for Phase 6
- `printed_at = datetime('now')` written alongside `status='printed'`
- No `'printing'` intermediate state (Phase 6)
- No `'failed'` writes in Phase 5
- Phase 5 does NOT write to `retry_queue`

**D-09 (Ack failure policy):** Order: `UPDATE status='printed'` THEN `ack_job()` (C4 constraint). If ack POST fails: log, leave status as `'printed'`. Phase 6 pending pull handles recovery. No inline ack retry in Phase 5.

### Claude's Discretion

- Exact function signature and internal structure of `run_print_worker()` — mirrors `run_pusher_loop()` but planner decides on logging verbosity and error formatting.
- Whether to use `while let Some(event) = rx.recv().await` or `loop { ... }` in the worker.
- Error handling granularity in `fetch_job_bytes()` — `anyhow::Result` is sufficient.
- SQL update helper — inline `conn.execute()` or a small `update_job_status()` helper function.
- Test coverage — unit tests for `fetch_job_bytes()` deser + ack 409 handling; integration test via `BREVLY_FAKE_PUSHER_EVENT` env var plus a fake HTTP server or mock.

### Deferred Ideas (OUT OF SCOPE)

- `status='printing'` intermediate fence — Phase 6 (RES-04)
- Inline ack retry — Phase 6 pending pull covers it
- Fetch failure typed errors (404 vs transport) — Phase 6 retry semantics
- Toast notification on print failure — Phase 6 (RES-02), Phase 5 only logs to stderr
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| PRT-01 | Agent fetches ESC/POS bytes via authenticated HTTP on event receipt | `fetch_job_bytes()` pattern confirmed from `pusher_auth()` template; base64 decode via `base64::engine::general_purpose::STANDARD.decode()` |
| PRT-02 | Agent prints order ticket (pedido novo confirmado) | All ticket types use same fetch→print path; routed by `enabled_types` filter only |
| PRT-03 | Agent prints dispatch ticket with QR code (already in ESC/POS bytes from Noren) | Same path as PRT-02; QR embedded server-side |
| PRT-04 | Agent prints closing receipt (cupom de fechamento) | Same path as PRT-02 |
| PRT-05 | Agent prints via Windows spooler (WritePrinter RAW) AND serial COM port | `printer_from_entry()` + `Box<dyn Printer>` already abstracts both paths; Phase 5 calls `print_raw()` only |
| PRT-06 | Ticket prints in < 1 second from event arrival | No intermediate queuing between mpsc recv and print_raw; fetch must be fast — single HTTP round-trip; print_raw is synchronous Win32 call |
| PRT-07 | Persistent SQLite dedup prevents double-print on reconnect/redelivery/crash | Phase 4 owns `INSERT OR IGNORE` fence (C3); Phase 5 updates `status='printed'` — second mpsc event for same job_id never arrives because Phase 4 skips it; dedup is at the fence, not in Phase 5 |
| PRT-08 | Ack sent only after `status='printed'` written to SQLite | D-09: UPDATE → ack_job() order enforced; crash between print and ack → job stays `'pending'` → Phase 6 re-delivers → C3 fence deduplicates at re-insert |
| PRT-09 | Agent respects per-tenant enabled_types flags | D-07: read from ConfigStore at startup; filter per event; disabled jobs marked `'printed'` and acked without printing |
</phase_requirements>

---

## Summary

Phase 5 closes the end-to-end print loop: the Phase 4 Pusher task already delivers `PrintEvent`s via an `mpsc` channel and writes the C3 dedup row to `printed_jobs` with `status='pending'`. Phase 5 picks up those events, fetches the ESC/POS byte payload from Noren via authenticated HTTP, dispatches it through the existing `Box<dyn Printer>` abstraction, marks the job done in SQLite, and fires the ack POST — in that order (D-09 / C4 constraint).

All infrastructure Phase 5 needs already exists: the `Printer` trait (`src/printer/mod.rs`), the WAL-mode second-connection pattern (`src/pusher/client.rs`), the `noren_client.rs` HTTP pattern (`pusher_auth()`), and the `config_store::get()` API for reading `enabled_types` / printer config. Phase 5 adds one new module (`src/print_worker.rs`), two new functions in `noren_client.rs`, and one new Cargo dependency (`base64 = "0.22"`).

The only external blocker is Noren shipping `GET /api/agent/jobs/{jobId}/bytes` and `POST /api/agent/jobs/{jobId}/ack`. The existing `BREVLY_FAKE_PUSHER_EVENT` dev shim plus the established mock TCP stub pattern (from `tests/noren_client_test.rs` and `tests/pusher_auth_test.rs`) lets the full pipeline be tested end-to-end on Linux without live Noren endpoints.

**Primary recommendation:** Implement `print_worker.rs` as a direct structural mirror of `pusher/client.rs` — same WAL-mode open, same `while let Some(event) = rx.recv().await` loop, same `eprintln!("[brevly-print] ...")` logging style. Two new functions in `noren_client.rs` using the `pusher_auth()` template. One dependency addition. Tests via the established TCP stub pattern.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Print event consumption | Async task (`print_worker`) | — | mpsc receiver owned by the worker; event loop stays unblocked |
| ESC/POS byte fetch | `noren_client` (HTTP) | — | Noren is the single source of truth for rendered bytes |
| Base64 decode | `print_worker` inline | `noren_client::fetch_job_bytes` | Decode happens inside `fetch_job_bytes` before returning `Vec<u8>` to caller |
| enabled_types filter | `print_worker` startup + per-event | `config_store` | Read once at startup, applied per event (D-07) |
| Print dispatch | `printer::Printer` trait | `spooler.rs` / `serial.rs` | Abstraction already built; Phase 5 calls `print_raw(&bytes)` only |
| SQLite status update | `print_worker` inline | `config_store` schema | `UPDATE printed_jobs SET status='printed'` — must precede ack (C4) |
| Ack delivery | `noren_client::ack_job` | — | Simple POST; 409 is `Ok(())` — idempotent by design |
| Health signalling | Not Phase 5 | Phase 4 / Phase 6 | Print worker never touches tray (C2 constraint) |

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tokio` | 1.x (already in project) | Async runtime hosting the print worker task | De facto Rust async runtime; already in use for Pusher task |
| `reqwest` | 0.13.4 (already in project) | HTTP client for `fetch_job_bytes` and `ack_job` | Already used for `activate()` and `pusher_auth()`; shared client instance |
| `rusqlite` | 0.40 (already in project) | Second WAL-mode SQLite connection in the print worker | Already used for C3 dedup fence in Pusher task; same pattern |
| `base64` | 0.22.1 | Decode base64-encoded ESC/POS bytes from Noren response | Standard crate; already transitive dep via `aws-lc-sys` in lockfile |
| `anyhow` | 1.x (already in project) | Error handling in new `noren_client` functions | Consistent with existing codebase pattern |
| `serde` / `serde_json` | 1.x (already in project) | Deserialize `{"bytes": "<base64>"}` response | Already used throughout |

[VERIFIED: crates.io registry] — `base64 = "0.22.1"` confirmed in `Cargo.lock` (transitive dep); `cargo search base64` confirmed version 0.22.1 on registry.

### Supporting (no new additions needed)

All supporting infrastructure is carried from prior phases:
- `printer_from_entry(&PrinterId)` — constructs `Box<dyn Printer>` for spooler or serial path
- `config_store::get()` — reads `enabled_types`, `printer_name`, `printer_type`
- `pusher::protocol::PrintEvent` — already defined struct with `job_id` and `job_type`

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `base64::engine::general_purpose::STANDARD` | `base64::prelude::BASE64_STANDARD` (alias) | Either works in 0.22; `STANDARD` engine is more explicit and matches docs |
| Inline `conn.execute()` for status update | Helper `update_job_status(conn, job_id, status)` | Helper reduces repetition if Phase 6 also needs it; planner's discretion (D-08 Claude's discretion) |
| `while let Some(event) = rx.recv().await` | `loop { match rx.recv().await { ... } }` | `while let` is idiomatic and cleaner; both are valid (D-03 Claude's discretion) |

**Installation (one new dependency):**
```bash
# Add to [dependencies] in Cargo.toml (NOT Windows-only — pure Rust, portable):
# base64 = "0.22"
cargo add base64@0.22
```

Note: `base64 = "0.22.1"` is already in `Cargo.lock` as a transitive dependency (pulled by `aws-lc-sys`). Adding it explicitly to `[dependencies]` just makes the direct dependency explicit and pins the version.

---

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `base64` | crates.io | ~11 yrs | Very high (transitive dep in most TLS stacks) | github.com/marshallpierce/rust-base64 | N/A — slopcheck unavailable | Approved [ASSUMED] |

**slopcheck was unavailable at research time.** However, `base64` is already present in `Cargo.lock` as a transitive dependency (confirmed by `grep "base64" Cargo.lock`), meaning it is already vetted through the existing dependency tree. Adding it as an explicit `[dependencies]` entry introduces no new code — the same version (0.22.1) is already compiled.

**Packages removed due to slopcheck [SLOP] verdict:** none

**Packages flagged as suspicious [SUS]:** none

*Note: All packages above are tagged `[ASSUMED]` because slopcheck was unavailable. The planner may skip `checkpoint:human-verify` for `base64` given it is already a transitive dependency in the current build, but may add one at their discretion.*

---

## Architecture Patterns

### System Architecture Diagram

```text
Pusher Task (Phase 4)
  │
  │  mpsc::Sender<PrintEvent>
  ▼
mpsc channel (buffer=32)
  │
  │  mpsc::Receiver<PrintEvent>  (consumed at spawn, not held in App)
  ▼
run_print_worker() ── tokio::spawn in main.rs Runtime block
  │
  ├─ Startup:
  │    open SQLite connection (WAL mode, second connection)
  │    config_store::get("enabled_types") → Vec<String>
  │    config_store::get("printer_name") + config_store::get("printer_type")
  │    → printer_from_entry(&PrinterId) → Box<dyn Printer>
  │
  └─ Event loop: while let Some(event) = rx.recv().await
       │
       ├─ enabled_types filter (D-07)
       │    ├─ job_type NOT in enabled_types
       │    │    UPDATE printed_jobs SET status='printed', printed_at=datetime('now')
       │    │    ack_job() → POST /api/agent/jobs/{jobId}/ack
       │    │    (skip print, no error)
       │    │
       │    └─ job_type IN enabled_types (or enabled_types empty)
       │         │
       │         ├─ fetch_job_bytes() → GET /api/agent/jobs/{jobId}/bytes
       │         │    → {"bytes": "<base64>"} → base64::STANDARD.decode() → Vec<u8>
       │         │    (on error: log, leave 'pending', continue — Phase 6 retries)
       │         │
       │         ├─ printer.print_raw(&bytes)
       │         │    ├─ Spooler path: WindowsSpoolerPrinter (WritePrinter RAW, C1)
       │         │    └─ Serial path: SerialPrinter (COM port write)
       │         │    (on error: log, leave 'pending', continue — Phase 6 retries)
       │         │
       │         ├─ UPDATE printed_jobs SET status='printed', printed_at=datetime('now')
       │         │    WHERE job_id=?  (BEFORE ack — C4 constraint, D-09)
       │         │
       │         └─ ack_job() → POST /api/agent/jobs/{jobId}/ack
       │              200 → Ok(())   409 → Ok(())   other → log + continue
       │
       rx.recv() returns None → Pusher task exited → worker exits
```

### Recommended Project Structure

```
src/
├── print_worker.rs        # NEW: run_print_worker() — mirrors pusher/client.rs structure
├── noren_client.rs        # EXTEND: add fetch_job_bytes() + ack_job() at bottom
├── lib.rs                 # EXTEND: add `pub mod print_worker;`
├── main.rs                # EXTEND: remove App._print_rx, spawn run_print_worker()
├── pusher/
│   └── client.rs          # UNCHANGED — template for print_worker.rs
├── printer/
│   └── mod.rs             # UNCHANGED — printer_from_entry() called from print_worker
└── config_store.rs        # UNCHANGED — config_store::get() called from print_worker
tests/
└── print_worker_test.rs   # NEW: fetch_job_bytes deser, ack 409 handling, mock TCP stubs
```

### Pattern 1: Print Worker Module Structure

**What:** A standalone `async fn run_print_worker(...)` that mirrors `run_pusher_loop()` exactly — WAL-mode connection open, `while let` receiver loop, `eprintln!` logging.

**When to use:** Always — this is the locked structure (D-01).

```rust
// src/print_worker.rs
// Source: mirrors src/pusher/client.rs run_pusher_loop() pattern
use std::path::PathBuf;
use tokio::sync::mpsc;
use crate::{
    config_store,
    noren_client::{fetch_job_bytes, ack_job},
    printer::{printer_from_entry, PrinterId, Printer},
    pusher::protocol::PrintEvent,
};

pub async fn run_print_worker(
    mut rx: mpsc::Receiver<PrintEvent>,
    agent_token: String,
    base_url: String,
    db_path: PathBuf,
    http: reqwest::Client,
) {
    // Open second SQLite connection (WAL mode — Pitfall 5 / D-03)
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[brevly-print] Print worker: failed to open SQLite connection: {e:#}");
            return;
        }
    };
    if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
        eprintln!("[brevly-print] Print worker: failed to set WAL mode: {e:#}");
        return;
    }

    // Read config at startup (D-03)
    let enabled_types: Vec<String> = /* config_store::get + serde_json::from_str */ vec![];
    let printer: Box<dyn Printer> = /* printer_from_entry */
        printer_from_entry(&PrinterId::Spooler("".into())); // placeholder

    // Event loop
    while let Some(event) = rx.recv().await {
        // D-07: enabled_types filter
        // D-08: UPDATE → ack order
        let _ = (event, &printer, &enabled_types, &conn, &http, &agent_token, &base_url);
    }
    eprintln!("[brevly-print] Print worker: channel closed — exiting");
}
```

### Pattern 2: `fetch_job_bytes()` in noren_client.rs

**What:** GET endpoint returning base64-encoded ESC/POS bytes — mirrors `pusher_auth()`.

**When to use:** Called once per event in the print worker.

```rust
// src/noren_client.rs — new function at bottom of file
// Source: mirrors pusher_auth() pattern; D-04 specifies exact API shape

use base64::Engine as _;

/// GET /api/agent/jobs/{jobId}/bytes → base64-decode → Vec<u8>
///
/// Response JSON: {"bytes": "<base64>"}
/// Security: agent_token passed via bearer_auth, never logged (T-02-02).
pub async fn fetch_job_bytes(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    job_id: &str,
) -> anyhow::Result<Vec<u8>> {
    #[derive(serde::Deserialize)]
    struct BytesResponse { bytes: String }

    let url = format!("{base_url}/api/agent/jobs/{job_id}/bytes");
    let resp = client
        .get(&url)
        .bearer_auth(agent_token)
        .send()
        .await
        .context("fetch_job_bytes: HTTP transport error")?;

    match resp.status().as_u16() {
        200 => {
            let body: BytesResponse = resp.json().await
                .context("fetch_job_bytes: response parse error")?;
            base64::engine::general_purpose::STANDARD
                .decode(&body.bytes)
                .context("fetch_job_bytes: base64 decode error")
        }
        status => anyhow::bail!("fetch_job_bytes: unexpected status {status}"),
    }
}
```

### Pattern 3: `ack_job()` in noren_client.rs

**What:** POST ack with idempotent 409 handling — per D-04.

```rust
/// POST /api/agent/jobs/{jobId}/ack (idempotent — 409 is Ok(()))
///
/// 200 → Ok(()); 409 → Ok(()) (already acked — C4 pitfall: normal after crash);
/// others → Err (logged by caller, not retried in Phase 5).
pub async fn ack_job(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    job_id: &str,
) -> anyhow::Result<()> {
    let url = format!("{base_url}/api/agent/jobs/{job_id}/ack");
    let resp = client
        .post(&url)
        .bearer_auth(agent_token)
        .send()
        .await
        .context("ack_job: HTTP transport error")?;

    match resp.status().as_u16() {
        200 | 409 => Ok(()),  // 409 = already acked (idempotent by design, C4)
        status => anyhow::bail!("ack_job: unexpected status {status}"),
    }
}
```

### Pattern 4: main.rs Changes

**What:** Remove `App._print_rx`, spawn `run_print_worker` after the Pusher spawn.

```rust
// main.rs — BEFORE (Phase 4 placeholder):
// _print_rx: Option<tokio::sync::mpsc::Receiver<PrintEvent>>,
//
// main.rs — AFTER (Phase 5):
// 1. Remove the _print_rx field from App struct
// 2. Remove `_print_rx: Some(print_rx)` from App initializer
// 3. Add after the Pusher spawn block:

if is_runtime {
    // ... Pusher spawn (unchanged) ...

    // Phase 5: spawn print worker (D-01)
    let worker_db_path = db_path.clone();
    let worker_http = http.clone();
    let worker_token = agent_token.clone();
    let worker_base_url = auth_url.clone(); // or noren_base_url()
    rt_handle.spawn(async move {
        brevly_print::print_worker::run_print_worker(
            print_rx, worker_token, worker_base_url, worker_db_path, worker_http
        ).await;
    });
    // Note: print_rx is moved into the task; no longer needed in App
}

// App initializer: omit _print_rx field entirely
```

### Pattern 5: SQLite Update (C4 order)

**What:** Status update must precede ack — the C4 critical ordering constraint.

```rust
// In print_worker.rs — after successful print_raw():
conn.execute(
    "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
    rusqlite::params![event.job_id],
)?;
// Only AFTER successful UPDATE does ack_job() fire:
if let Err(e) = ack_job(&http, &base_url, &agent_token, &event.job_id).await {
    eprintln!("[brevly-print] Print worker: ack failed for {}: {e:#}", event.job_id);
    // D-09: leave status 'printed'; Phase 6 pending pull covers recovery
}
```

### Anti-Patterns to Avoid

- **Ack before UPDATE:** Violates C4 — if process crashes between ack and UPDATE, the job appears done in Noren but not in local SQLite. Phase 6 pending pull won't re-deliver it (Noren won't resend acked jobs). Job is silently lost.
- **Storing `print_rx` in `App`:** Phase 4 left it as `Option` — Phase 5 removes it entirely. Keeping it means the channel stays alive but unread; the Pusher task's `tx.send()` never blocks, but events accumulate in the buffer until it fills (32 capacity), then Pusher task backs up. Remove it from `App` as specified in D-02.
- **Blocking the event loop with print_raw():** `printer.print_raw()` is synchronous Win32/serial I/O. It must run in the `tokio::spawn`'d print worker task (already async via `rt_handle.spawn`), never directly in the winit event loop handler.
- **Logging `agent_token`:** Bearer token must never appear in `eprintln!` output — T-02-02. Use `.bearer_auth()` and never format the token into any log or error message.
- **Opening SQLite without WAL:** Two concurrent connections (main App + print worker) without WAL mode → `SQLITE_BUSY` under concurrent writes. WAL allows concurrent readers and one writer (Pitfall 5 from Phase 1 research).
- **Reusing a single SQLite connection across tasks:** `rusqlite::Connection` is not `Send`. Each task must open its own connection (D-03 pattern, same as Pusher task).
- **Re-creating `reqwest::Client` per call:** Build the client once in `main()`, clone the handle. Already established — pass `http: reqwest::Client` into `run_print_worker()`.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Base64 decode | Custom decode loop | `base64::engine::general_purpose::STANDARD.decode()` | Padding edge cases, alphabet variants, performance |
| HTTP bearer auth | Manual `Authorization` header | `.bearer_auth(token)` on reqwest builder | Handles `Bearer ` prefix, avoids token in format strings |
| SQLite concurrent access | Custom locking / mutex around connection | WAL mode + separate connection per task | WAL is SQLite's built-in concurrent-writer support; one `rusqlite::Connection` per task is the pattern |
| Printer dispatch routing | `match event.job_type { ... }` for different print paths | Single `printer.print_raw(&bytes)` call | All ticket types use the same ESC/POS byte path; Noren owns the rendering |
| Dedup logic in print worker | Check-then-insert in Phase 5 | Rely on Phase 4's `INSERT OR IGNORE` fence | Phase 4 already owns C3; Phase 5 receives pre-deduped events via mpsc |

**Key insight:** Phase 5 is intentionally a "dumb spooler" — it dispatches bytes it receives from Noren without routing by ticket type, interpreting ESC/POS content, or managing its own dedup. All complexity lives upstream (Noren renders, Phase 4 deduplicates) or downstream (Phase 6 retries).

---

## Common Pitfalls

### Pitfall 1: Ack Order Violation (C4)
**What goes wrong:** `ack_job()` fires before `UPDATE status='printed'`. Process crashes between ack and update. SQLite shows `'pending'` but Noren will not re-deliver (already acked). Job is silently lost.
**Why it happens:** Developer instinct to send confirmation immediately after successful print; the UPDATE seems like a follow-up step.
**How to avoid:** Strictly enforce D-09 order: UPDATE → ack_job(). Every code path that calls `ack_job()` must call UPDATE first.
**Warning signs:** Any code path where `ack_job()` can return before the UPDATE executes.

### Pitfall 2: RAW Datatype Not Propagated (C1)
**What goes wrong:** Phase 2 validated WritePrinter with `pDatatype="RAW"` in the test-print button. If Phase 5 alters the printer call path or calls a different spooler function, ESC/POS bytes arrive at the printer as garbage (rendered as text).
**Why it happens:** Adding an intermediate layer that changes the data type argument.
**How to avoid:** Phase 5 calls `printer.print_raw(&bytes)` only — the `"RAW"` type is hardcoded inside `WindowsSpoolerPrinter::print_raw()`. Do not touch `spooler.rs`.
**Warning signs:** Thermal printer outputs ASCII characters instead of formatted receipt.

### Pitfall 3: App._print_rx Not Removed
**What goes wrong:** Phase 4 placeholder `_print_rx: Option<Receiver<PrintEvent>>` stays in `App`. Phase 5 spawns the worker with a clone of the receiver — but mpsc receivers cannot be cloned. The receiver must be moved into the task, not cloned.
**Why it happens:** `App._print_rx` holds the only receiver. Moving it into the worker task requires removing it from `App`.
**How to avoid:** D-02 specifies removing `App._print_rx` entirely. Move `print_rx` directly into the `rt_handle.spawn(async move { ... })` block.
**Warning signs:** Compiler error "value moved here but used later" or "mpsc::Receiver does not implement Clone".

### Pitfall 4: Accessing Tray from Print Worker (C2)
**What goes wrong:** Print worker tries to update tray state (e.g., health indicator) directly from its async task.
**Why it happens:** Natural desire to show red tray icon on print failure.
**How to avoid:** Phase 5 logs to stderr only (D-09, deferred); Phase 6 owns toast notifications. Print worker never holds an `EventLoopProxy` or tray handle. Health signalling from background tasks must go through `proxy.send_event(UserEvent::HealthChanged(...))` — but Phase 5 doesn't do this either (Phase 6's concern).
**Warning signs:** Compile error about `Send` bounds on `EventLoopProxy` or `TrayIcon`.

### Pitfall 5: `enabled_types` JSON Deserialization Edge Cases
**What goes wrong:** ConfigStore stores `enabled_types` as JSON string (e.g., `["order","dispatch","closing"]`). If `serde_json::from_str::<Vec<String>>()` fails (empty string, malformed JSON), the filter crashes the worker or panics.
**Why it happens:** Missing key or empty value from ConfigStore returns `None` or `""`.
**How to avoid:** D-03 specifies: if `enabled_types` is missing or empty → default to allow-all (fail-safe). Use `Option::and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()`.
**Warning signs:** Print worker exits on startup with a JSON parse error.

### Pitfall 6: base64 API Changes Between 0.21 and 0.22
**What goes wrong:** `base64` 0.21 used `base64::decode(s)` (deprecated free functions). `base64` 0.22 requires the `Engine` trait: `use base64::Engine as _; base64::engine::general_purpose::STANDARD.decode(s)`.
**Why it happens:** Training data / examples from before 0.22 use old API.
**How to avoid:** Always use the Engine API in 0.22. The old free functions are removed in 0.22.
**Warning signs:** Compiler error "function `base64::decode` not found"; use `STANDARD.decode()` instead.

### Pitfall 7: print_raw() Latency on Serial Path
**What goes wrong:** Serial COM port writes can block for the duration of the data transmission at the port's baud rate. For a large cupom de fechamento, this could exceed the < 1 second requirement (PRT-06).
**Why it happens:** `serialport::write()` is synchronous; the print worker is in an async task but the write blocks the task thread.
**How to avoid:** `tokio::task::spawn_blocking` wraps the synchronous `print_raw()` call if latency measurements show it blocks. However, Phase 2 tested `print_raw()` in the test-print button and it passed; check only if PRT-06 is failing. The `StubPrinter` on Linux masks this.
**Warning signs:** PRT-06 criterion failing in Windows testing (> 1 second from event to print); use `spawn_blocking` as the fix.

---

## Code Examples

### fetch_job_bytes full implementation
```rust
// Source: D-04 (CONTEXT.md) + pusher_auth() pattern in noren_client.rs [ASSUMED API shape; verified pattern]
use anyhow::Context as _;
use base64::Engine as _;

pub async fn fetch_job_bytes(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    job_id: &str,
) -> anyhow::Result<Vec<u8>> {
    #[derive(serde::Deserialize)]
    struct BytesResponse { bytes: String }

    let url = format!("{base_url}/api/agent/jobs/{job_id}/bytes");
    let resp = client
        .get(&url)
        .bearer_auth(agent_token)  // Never logs the token (T-02-02)
        .send()
        .await
        .context("fetch_job_bytes: HTTP transport error")?;

    match resp.status().as_u16() {
        200 => {
            let body: BytesResponse = resp
                .json()
                .await
                .context("fetch_job_bytes: response parse error")?;
            base64::engine::general_purpose::STANDARD
                .decode(&body.bytes)
                .context("fetch_job_bytes: base64 decode error")
        }
        status => anyhow::bail!("fetch_job_bytes: unexpected status {status}"),
    }
}
```

### enabled_types filter (fail-safe)
```rust
// Source: D-03 + D-07 (CONTEXT.md) [ASSUMED code shape; verified logic]
let enabled_types: Vec<String> = config_store::get(&conn, "enabled_types")
    .unwrap_or(None)
    .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
    .unwrap_or_default(); // empty = allow all (fail-safe, D-03)

// Per-event check:
if !enabled_types.is_empty() && !enabled_types.contains(&event.job_type) {
    // Disabled type: mark printed + ack, skip print (D-07, PRT-09)
    let _ = conn.execute(
        "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
        rusqlite::params![event.job_id],
    );
    let _ = ack_job(&http, &base_url, &agent_token, &event.job_id).await;
    continue;
}
```

### Test: fetch_job_bytes mock HTTP stub
```rust
// Source: tests/noren_client_test.rs spawn_stub pattern (verified, exists in codebase)
// tests/print_worker_test.rs

#[tokio::test]
async fn test_fetch_job_bytes_200_decodes_base64() {
    use base64::Engine as _;
    let raw = b"\x1b\x40Hello\x1d\x56\x00";
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
    let body_json = format!(r#"{{"bytes":"{}"}}"#, encoded);
    let body: &'static str = Box::leak(body_json.into_boxed_str());

    let base_url = spawn_stub(200, body).await;
    let client = reqwest::Client::new();
    let result = fetch_job_bytes(&client, &base_url, "tok-test", "job-001").await;
    assert_eq!(result.unwrap(), raw);
}

#[tokio::test]
async fn test_ack_job_409_returns_ok() {
    let base_url = spawn_stub(409, r#"{"error":"already acked"}"#).await;
    let client = reqwest::Client::new();
    let result = ack_job(&client, &base_url, "tok-test", "job-001").await;
    assert!(result.is_ok(), "409 must be Ok(()) — idempotent by design (C4)");
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `base64::decode(s)` (free function) | `base64::engine::general_purpose::STANDARD.decode(s)` (Engine trait) | base64 0.22 (2023) | Free functions removed; must use Engine API |
| `base64::encode(s)` | `STANDARD.encode(s)` | base64 0.22 | Same — Engine trait required |
| Single SQLite connection + mutex | WAL mode + one connection per task | Established in Phase 1 research | Eliminates SQLITE_BUSY deadlock under concurrent async tasks |

**Deprecated/outdated:**
- `base64::decode()` / `base64::encode()` free functions: removed in base64 0.22. Any training data or StackOverflow examples using these are wrong for this project.

---

## Runtime State Inventory

> Phase 5 is not a rename/refactor/migration phase. This section is omitted.

---

## Open Questions

1. **Job type string verification (D-06 — MUST verify before ship)**
   - What we know: CONTEXT.md expects `"order"`, `"dispatch"`, `"closing"`, `"cancel"` to match what Noren emits in the Pusher event `type` field.
   - What's unclear: Noren's actual emit code hasn't been grepped — the strings might differ (e.g., camelCase, Portuguese names, or snake_case).
   - Recommendation: Grep `~/repos/brevly/noren` for the Pusher trigger call and the `type` field value before merging Phase 5. This is a blocking verification step, not deferred.

2. **Noren endpoint live status**
   - What we know: `GET /api/agent/jobs/{jobId}/bytes` and `POST /api/agent/jobs/{jobId}/ack` are listed as external blockers in STATE.md.
   - What's unclear: Whether these endpoints exist yet in the Noren repo at time of Phase 5 planning.
   - Recommendation: Phase 5 implementation can proceed with mock TCP stubs (proven pattern from Phase 2/4 tests). Wire against live endpoints when Noren ships them.

3. **`base_url` parameter in `run_print_worker`**
   - What we know: `main.rs` reads `noren_base_url` from ConfigStore with `unwrap_or_else(noren_base_url)` and stores it as `auth_url` in `PusherConfig`. The same URL is needed for `fetch_job_bytes` and `ack_job`.
   - What's unclear: Whether to re-read from ConfigStore inside `run_print_worker` or receive it as a parameter from `main.rs`.
   - Recommendation: Pass `base_url: String` as a parameter (same approach as `agent_token`); `main.rs` already has it as `auth_url` in the Pusher spawn block. Planner's discretion (D-03 Claude's discretion).

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | Compilation | ✓ | rustc 1.97.0 (2026-07-07) | — |
| `cargo test` | Test runner | ✓ | cargo 1.97.0 | — |
| `base64` crate | `fetch_job_bytes` | ✓ (transitive) | 0.22.1 in Cargo.lock | — |
| Noren `GET /api/agent/jobs/{jobId}/bytes` | PRT-01 integration | ✗ (not yet live) | — | Mock TCP stub (tests/print_worker_test.rs) |
| Noren `POST /api/agent/jobs/{jobId}/ack` | PRT-08 integration | ✗ (not yet live) | — | Mock TCP stub |
| Thermal printer (USB/serial) | PRT-02..06 | ✗ (Linux CI) | — | StubPrinter (always Ok(())) on Linux |

**Missing dependencies with no fallback:** None — all blockers have test fallbacks.

**Missing dependencies with fallback:**
- Noren endpoints not yet live → mock TCP stub pattern (established in Phase 2 and 4 tests, fully reusable for Phase 5)
- Thermal printer not on Linux CI → `StubPrinter` returns `Ok(())`, printer abstraction already handles this

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in (`#[test]`, `#[tokio::test]`) |
| Config file | none (Cargo default) |
| Quick run command | `cargo test --lib -q` |
| Full suite command | `cargo test -q` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PRT-01 | `fetch_job_bytes` decodes base64 response correctly | unit | `cargo test -q fetch_job_bytes` | ❌ Wave 0 |
| PRT-01 | `fetch_job_bytes` handles non-200 status as Err | unit | `cargo test -q fetch_job_bytes` | ❌ Wave 0 |
| PRT-07 | Dedup: second delivery of same job_id skipped by Phase 4 fence (verified in Phase 4 tests) | unit | `cargo test -q insert_print_job` | ✅ `src/pusher/client.rs` tests |
| PRT-08 | `ack_job` 409 response returns `Ok(())` | unit | `cargo test -q ack_job` | ❌ Wave 0 |
| PRT-08 | SQLite UPDATE precedes ack_job call (ordering verified by test mock) | integration | `cargo test -q print_worker_test` | ❌ Wave 0 |
| PRT-09 | Disabled job_type: UPDATE to 'printed' + ack, no print | unit | `cargo test -q enabled_types_filter` | ❌ Wave 0 |
| PRT-02/03/04 | All job types routed through same print path | unit | `cargo test -q print_worker_test` | ❌ Wave 0 |
| PRT-05 | StubPrinter returns Ok(()) on Linux for both Spooler and Serial ids | unit | `cargo test printer_test` | ✅ `tests/printer_test.rs` |
| PRT-06 | < 1 second latency | manual | Windows integration test | ❌ manual-only (no hardware on Linux CI) |

### Sampling Rate
- **Per task commit:** `cargo test --lib -q`
- **Per wave merge:** `cargo test -q`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `tests/print_worker_test.rs` — covers PRT-01, PRT-08, PRT-09, fetch_job_bytes, ack_job mock stubs
- [ ] `src/print_worker.rs` — the module itself (does not exist yet)

---

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | Print worker is an internal task; auth is via agent_token Bearer header on HTTP calls — already established in prior phases |
| V3 Session Management | no | No session; stateless HTTP calls per job |
| V4 Access Control | no | Internal task; no user-facing access decision |
| V5 Input Validation | yes | base64 decode input from Noren response; `serde_json` parse of response body — both use typed deserialization |
| V6 Cryptography | no | No new crypto; Bearer token from DPAPI store (Phase 2) passed through |

### Known Threat Patterns for Rust async HTTP client + SQLite

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| ESC/POS payload injection (malicious Noren response) | Tampering | Agent is a dumb spooler — bytes passed to printer verbatim; no injection surface in Rust. Noren is trusted server. |
| SQL injection in job_id | Tampering | Parameterised queries via `rusqlite::params![]` — already established pattern in Phase 4 insert_print_job |
| Token leakage in logs | Information Disclosure | T-02-02 enforced: `bearer_auth()` only, never format token into `eprintln!` strings |
| 409 treated as error → retry storm | Denial of Service | D-04 specifies 409 is `Ok(())` — prevents spurious retry on idempotent ack |

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Job type strings are `"order"`, `"dispatch"`, `"closing"`, `"cancel"` | D-06 / enabled_types filter | enabled_types filter silently passes or blocks all jobs if strings don't match Noren's actual values; MUST VERIFY before ship |
| A2 | `GET /api/agent/jobs/{jobId}/bytes` response JSON is `{"bytes": "<base64>"}` | D-04 / fetch_job_bytes response shape | `BytesResponse` deserialization fails at runtime if field name differs |
| A3 | `POST /api/agent/jobs/{jobId}/ack` returns 200 on success and 409 on repeat | D-04 / ack_job status mapping | 409 handling returns `Ok(())` — if Noren uses a different code (e.g., 200 always), no functional difference; if it uses 4xx for error, ack_job would surface false errors |
| A4 | `base64` crate [ASSUMED] — slopcheck unavailable; however, it is already in Cargo.lock as transitive dep | Package Legitimacy Audit | Low risk: already compiled in this project's dependency tree |
| A5 | `noren_base_url` available as `auth_url` in main.rs Runtime block, passable to run_print_worker as a String | main.rs integration | If variable scope differs, planner will need to re-read from ConfigStore inside the worker |

**If this table is empty:** All claims in this research were verified or cited — no user confirmation needed.

---

## Sources

### Primary (HIGH confidence)
- `src/pusher/client.rs` — structural template for `run_print_worker()`, WAL-mode pattern, logging style [VERIFIED: codebase]
- `src/noren_client.rs` — `pusher_auth()` pattern for new HTTP functions [VERIFIED: codebase]
- `src/printer/mod.rs` — `printer_from_entry()`, `Printer` trait, `PrinterId` [VERIFIED: codebase]
- `src/config_store.rs` — `config_store::get()` API, WAL-mode open [VERIFIED: codebase]
- `src/main.rs` — App struct, mpsc channel wiring, Runtime spawn block [VERIFIED: codebase]
- `Cargo.lock` — base64 0.22.1 already in transitive deps [VERIFIED: codebase]
- `.planning/phases/05-job-pipeline/05-CONTEXT.md` — all D-01..D-09 decisions [VERIFIED: planning artifact]
- `tests/noren_client_test.rs`, `tests/pusher_auth_test.rs` — mock TCP stub pattern [VERIFIED: codebase]

### Secondary (MEDIUM confidence)
- `cargo search base64` + Cargo.lock inspection — base64 0.22.1 confirmed on crates.io registry [VERIFIED: crates.io via cargo search]
- base64 0.22 Engine API (`use base64::Engine as _; STANDARD.decode()`) [ASSUMED: based on training knowledge; confirmed by presence of 0.22.1 in lockfile]

### Tertiary (LOW confidence)
- Noren endpoint response shapes (`{"bytes": "<base64>"}`, 409 on repeat ack) — specified in CONTEXT.md but not yet verifiable against live endpoints [ASSUMED: per CONTEXT.md D-04 / Specifics section]

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all dependencies already in project; only `base64` is new (already transitive)
- Architecture: HIGH — `run_print_worker` is a direct structural mirror of the existing `run_pusher_loop`; all infrastructure built in prior phases
- Pitfalls: HIGH — C1, C2, C3, C4 constraints come from verified prior phase research and are cross-referenced in CONTEXT.md

**Research date:** 2026-07-16
**Valid until:** 2026-08-16 (stable Rust ecosystem; Noren endpoint shapes may change before they go live)
