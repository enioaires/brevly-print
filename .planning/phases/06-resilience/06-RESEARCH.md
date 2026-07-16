# Phase 6: Resilience - Research

**Researched:** 2026-07-16
**Domain:** Rust async task architecture — retry scheduling, crash recovery, SQLite schema migration, Windows toast notifications, Pusher reconnect integration
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

All decisions below are locked. The owner delegated all decisions ("pode decidir tudo e já criar o contexto, não entendo muito sobre a parte técnica") — same posture as prior phases. Treat every D-XX below as decided.

- **D-01:** Migration v2 uses table-recreation pattern (SQLite cannot `ALTER TABLE` to modify a CHECK constraint). Recreates `printed_jobs` as `printed_jobs_v2` with `'printing'` added to the CHECK constraint, inserts old data, drops old table, renames. Added as second `M::up` in `MIGRATIONS` vec in `config_store.rs`.
- **D-02:** `run_print_worker` adds `UPDATE printed_jobs SET status='printing'` BEFORE calling `printer.print_raw()` — the crash recovery fence (RES-04). On print failure, inserts into `retry_queue` instead of continuing (D-12).
- **D-03:** New `src/retry_task.rs` with `pub async fn run_retry_task(...)` — a fourth Tokio task. Signature takes `db_path`, `agent_token`, `base_url`, `http`, `printer: Box<dyn Printer + Send>`, and a `send_health` closure.
- **D-04:** The retry task opens its own SQLite connection (4th total) with `PRAGMA journal_mode=WAL`. Pattern identical to `run_print_worker` and `run_pusher_loop`.
- **D-05:** At startup, before entering the poll loop, the retry task performs crash recovery: queries `printed_jobs WHERE status='printing' AND job_id NOT IN (SELECT job_id FROM retry_queue)`, calls `fetch_job_bytes()` for each, inserts into `retry_queue` with `next_retry_at=datetime('now')` for immediate retry. On `fetch_job_bytes` failure: log and skip (idempotent on next boot).
- **D-06:** Poll loop with `tokio::time::interval(Duration::from_secs(5))`. Query: `SELECT job_id, job_type, escpos_bytes, attempt_count FROM retry_queue WHERE next_retry_at <= datetime('now') ORDER BY next_retry_at ASC LIMIT 10`. On success: `UPDATE status='printed'` → `ack_job()` → `DELETE FROM retry_queue` → `send_health(Connected)`. On failure < 3: increment `attempt_count`, set `next_retry_at=datetime('now', '+30 seconds')`. On failure >= 3: `UPDATE status='failed'` → `DELETE FROM retry_queue` → `send_health(Problem)` → toast (D-07).
- **D-07:** Toast on retry exhaustion using `tauri-winrt-notification` (already in `Cargo.toml`). Pattern: `Toast::new(Toast::POWERSHELL_APP_ID).title("Brevly Print — Falha na impressão").text1("Falha ao imprimir após 3 tentativas.").text2("Verifique se a impressora está ligada e com papel.").show()`. Wrapped in `#[cfg(windows)]`. On Linux: log to stderr.
- **D-08:** After `subscription_succeeded` + `send_health(Connected)` in `run_pusher_loop` (~line 290 of `pusher/client.rs`), add call to new `noren_client::fetch_pending_jobs()`. Iterates results, calls `insert_print_job()` (dedup fence), sends to print worker via `tx.try_send()` with spawn-on-Full fallback (WR-04 pattern). Error handling: log + continue, do not reconnect WebSocket.
- **D-09:** Field names for `PendingJob` struct TBD — planner must grep Noren repo for `/api/agent/jobs/pending` implementation. Use `#[serde(rename_all = "camelCase")]` if JS/TS backend uses camelCase (most likely).
- **D-10:** Update `health_state.rs` `Problem` variant strings: `tooltip()` → `"Brevly Print — Falha na impressora"`, `status_label()` → `"Falha na impressora"`.
- **D-11:** `run_print_worker` does NOT get `send_health`. Health state transitions on failure are owned exclusively by the retry task. Print worker only sets `status='printing'` fence and writes to `retry_queue`.
- **D-12:** `retry_queue` INSERT on failure: `INSERT OR IGNORE INTO retry_queue (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at) VALUES (?1, ?2, ?3, 1, datetime('now', '+30 seconds'), ?4, datetime('now'))`. BLOB binding: `bytes.as_slice()`.

### Claude's Discretion

- Exact SQL parameter binding style for BLOB (`&[u8]` vs `Vec<u8>` wrapper).
- Whether `run_retry_task` uses `tokio::time::interval` or `tokio::time::sleep` loop (interval is more predictable).
- Error handling granularity in `fetch_pending_jobs()` — `anyhow::Result<Vec<PendingJob>>` is sufficient.
- Test strategy: unit test for startup crash recovery query, unit test for retry exhaustion path (mock printer), integration test via `BREVLY_FAKE_PUSHER_EVENT` + mock HTTP stub.
- Whether retry task `LIMIT 10` per tick is tunable or hardcoded (hardcode 10).
- Logging verbosity for retry attempts.

### Deferred Ideas (OUT OF SCOPE)

- Heartbeat to Noren (`OBS-01`) — v2.
- Paper-level sensing via `DLE EOT` — v2, serial only.
- Typed retry errors (printer offline vs paper out vs USB disconnect) — v2.
- Retry count configurability — v2.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| RES-01 | Retry the print job 3× with 30s interval when printer fails | D-06 retry poll loop; `retry_queue` table already in schema v1; attempt_count tracking |
| RES-02 | Toast notification + red tray icon after 3 failed retries | `tauri-winrt-notification` already in `Cargo.toml` v0.8; D-07 pattern; `send_health(Problem)` closure already established |
| RES-03 | Pull unacked jobs from `/api/agent/jobs/pending` on Pusher reconnect | D-08 insert point in `pusher/client.rs` line ~290; `insert_print_job()` dedup fence reused unchanged |
| RES-04 | Boot crash recovery: reprocess jobs left in `status='printing'` | D-01 migration v2 adds 'printing' to CHECK; D-05 startup scan; SQLite dedup prevents double-print |
</phase_requirements>

---

## Summary

Phase 6 is a **surgical extension of existing infrastructure**: the schema, tables, HTTP client patterns, health state machine, and printer abstractions are all established. No new crates are needed — `tauri-winrt-notification` (already in `Cargo.toml`), `rusqlite`, `tokio`, `reqwest`, and `anyhow` cover 100% of the work.

The four requirements break into three independent work streams that share infrastructure but have no ordering dependencies among themselves after the schema migration:

1. **Retry path (RES-01/02/04):** New `retry_task.rs` + modifications to `print_worker.rs` and `config_store.rs`. The retry task is a fourth Tokio async task following the established spawn pattern in `main.rs`. SQLite's WAL mode supports four concurrent connections without additional Rust-level locking.

2. **Pending pull (RES-03):** Five-line addition to `pusher/client.rs` after the existing `send_health(Connected)` call. New `fetch_pending_jobs()` function in `noren_client.rs` follows the established `fetch_job_bytes()` pattern exactly.

3. **Health state update (D-10):** Two string literal changes in `health_state.rs`. No logic change — the `Problem` variant already exists; only the human-readable strings change.

The single cross-cutting prerequisite is migration v2 (D-01): it must run before any code that reads or writes `status='printing'`. Since migrations run at startup before any tasks are spawned, this ordering is guaranteed by the existing `open_and_migrate()` call in `main.rs`.

**Primary recommendation:** Implement in two plans — Plan 1: migration v2 + print_worker fence + retry_task (RES-01/02/04), Plan 2: pending pull + health string update (RES-03 + D-10). This keeps the external Noren dependency (Plan 2) isolated from the locally-testable retry path (Plan 1).

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Retry scheduling (RES-01) | Print agent — retry_task.rs | — | Printer failure is local; retry logic belongs in the agent, not Noren |
| Toast notification (RES-02) | Print agent — Windows WinRT | — | OS-level notification; no Noren involvement |
| Red tray on exhaustion (RES-02) | Print agent — tray icon via health closure | — | Existing HealthState::Problem path; just needs to be triggered |
| Pending pull on reconnect (RES-03) | Print agent — pusher/client.rs | Noren backend — GET /api/agent/jobs/pending | Agent initiates; Noren provides the data |
| Crash recovery fence (RES-04) | Print agent — SQLite printed_jobs | — | Local state machine; no Noren call needed for detection |
| SQLite schema migration | Print agent — config_store.rs | — | rusqlite_migration handles version tracking |

---

## Standard Stack

### Core (all already in Cargo.toml — no new dependencies)

| Library | Version in Cargo.toml | Purpose | Status |
|---------|----------------------|---------|--------|
| `rusqlite` | 0.40 (bundled) | 4th WAL connection, retry_queue reads/writes, migration v2 | Already used in print_worker, pusher/client |
| `rusqlite_migration` | 2.6 | Migration v2 (`M::up` second element in `MIGRATIONS` vec) | Already used in config_store |
| `tokio` | 1 (full) | `tokio::time::interval` (5s poll), `Duration::from_secs(30)` for next_retry_at | Already used |
| `reqwest` | 0.13 (rustls) | `fetch_pending_jobs()` HTTP GET | Already used |
| `anyhow` | 1 | `anyhow::Result<Vec<PendingJob>>` in `fetch_pending_jobs()` | Already used |
| `serde` + `serde_json` | 1 | `PendingJob` struct deserialization; `#[serde(rename_all = "camelCase")]` | Already used |
| `tauri-winrt-notification` | 0.8 | Windows toast on retry exhaustion (RES-02) | Already in `[target.'cfg(windows)'.dependencies]` |

**No new `Cargo.toml` entries needed for Phase 6.** [VERIFIED: Cargo.toml confirmed above]

### Installation

```bash
# No cargo add commands needed — all deps already present.
# cargo build will succeed without changes to Cargo.toml.
```

---

## Package Legitimacy Audit

> Phase 6 installs no new packages. All dependencies are already present in `Cargo.toml` and
> were validated in prior phases. No audit action required.

| Package | Registry | Status | Disposition |
|---------|----------|--------|-------------|
| `tauri-winrt-notification` | crates.io | v0.8 — already in Cargo.toml since Phase 3 | Approved (prior phase) |
| `rusqlite` | crates.io | v0.40 — already in Cargo.toml since Phase 1 | Approved (prior phase) |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

---

## Architecture Patterns

### System Architecture Diagram

```
Pusher WebSocket
  |
  | subscription_succeeded (every reconnect)
  v
run_pusher_loop [pusher/client.rs]
  |--- fetch_pending_jobs() --> GET /api/agent/jobs/pending [Noren]
  |      |
  |      v (PendingJob list)
  |    insert_print_job() [dedup fence] --> if new: tx.try_send(PrintEvent)
  |
  | print:job event
  v
  insert_print_job() --> tx.try_send(PrintEvent)

mpsc channel (capacity 32)
  |
  v
run_print_worker [print_worker.rs]
  |--- fetch_job_bytes() --> GET /api/agent/jobs/{id}/bytes [Noren]
  |--- UPDATE status='printing' [SQLite — crash fence D-02]
  |--- printer.print_raw()
  |    |
  |    | SUCCESS: UPDATE status='printed' --> ack_job() --> done
  |    |
  |    | FAILURE: INSERT OR IGNORE retry_queue (attempt=1, next_retry_at=now+30s)
  |              (status stays 'printing')

tokio::time::interval(5s)
  |
  v
run_retry_task [retry_task.rs]  <-- spawned in main.rs alongside pusher + print_worker
  |
  | STARTUP (crash recovery D-05):
  |   SELECT printed_jobs WHERE status='printing' AND job_id NOT IN retry_queue
  |   --> fetch_job_bytes() --> INSERT retry_queue (next_retry_at=now)
  |
  | POLL LOOP (every 5s):
  |   SELECT retry_queue WHERE next_retry_at <= now ORDER BY next_retry_at LIMIT 10
  |   for each row:
  |     UPDATE status='printing' [crash fence]
  |     printer.print_raw(escpos_bytes)
  |     |
  |     | SUCCESS: UPDATE status='printed' --> ack_job() --> DELETE retry_queue
  |     |          send_health(Connected) [green tray]
  |     |
  |     | FAILURE, attempt < 3: UPDATE retry_queue (attempt++, next_retry_at=now+30s)
  |     |
  |     | FAILURE, attempt >= 3:
  |               UPDATE status='failed'
  |               DELETE retry_queue
  |               send_health(Problem) --> red tray via EventLoopProxy
  |               Toast::show() [#[cfg(windows)]]

health closure (impl Fn(HealthState) + Send + 'static)
  |
  v
EventLoopProxy<UserEvent> --> UserEvent::HealthChanged --> tray_runtime::apply_health()
```

### Recommended Project Structure

```
src/
├── retry_task.rs        # NEW — fourth Tokio task (D-03)
├── print_worker.rs      # MODIFIED — add status='printing' fence + retry_queue insert
├── config_store.rs      # MODIFIED — add migration v2 (D-01)
├── health_state.rs      # MODIFIED — update Problem strings (D-10)
├── noren_client.rs      # MODIFIED — add fetch_pending_jobs() (D-08/D-09)
├── pusher/
│   └── client.rs        # MODIFIED — call fetch_pending_jobs() after subscription_succeeded
├── lib.rs               # MODIFIED — add `pub mod retry_task;`
└── main.rs              # MODIFIED — spawn retry task in is_runtime block
tests/
├── retry_task_test.rs   # NEW — crash recovery query, retry exhaustion path
└── [existing tests unchanged]
```

### Pattern 1: Fourth WAL Connection (D-04)

[ASSUMED] based on the established pattern in `print_worker.rs` and `pusher/client.rs`:

```rust
// In run_retry_task — identical boilerplate to run_print_worker
let conn = match rusqlite::Connection::open(&db_path) {
    Ok(c) => c,
    Err(e) => {
        eprintln!("[brevly-print] Retry task: failed to open SQLite connection: {e:#}");
        return;
    }
};
if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
    eprintln!("[brevly-print] Retry task: failed to set WAL mode: {e:#}");
    return;
}
```

**Why safe with 4 concurrent connections:** SQLite WAL mode allows unlimited concurrent readers and serializes writers via WAL file locking. The retry task is the only writer to `retry_queue`; `printed_jobs` may have concurrent writes from `print_worker` and `retry_task` — WAL's write lock serializes these at the SQLite level without Rust-level Mutex needed. [VERIFIED: config_store.rs comment + established pattern in print_worker.rs and pusher/client.rs]

### Pattern 2: BLOB Binding for escpos_bytes (D-12)

```rust
// INSERT into retry_queue with BLOB bytes
conn.execute(
    "INSERT OR IGNORE INTO retry_queue
        (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
     VALUES
        (?1, ?2, ?3, 1, datetime('now', '+30 seconds'), ?4, datetime('now'))",
    rusqlite::params![
        event.job_id,
        event.job_type,
        bytes.as_slice(),   // &[u8] binds as BLOB
        error_message,
    ],
)?;

// Reading back
let escpos_bytes: Vec<u8> = row.get(2)?;  // column index 2 = escpos_bytes
```

[VERIFIED: rusqlite params macro accepts `&[u8]` for BLOB binding — established pattern, `row.get::<_, Vec<u8>>()` for retrieval]

### Pattern 3: Health Closure for Retry Task (D-03)

```rust
// In main.rs — create proxy BEFORE spawning retry task (same as Pusher proxy)
let proxy_for_retry = event_loop.create_proxy();
let retry_send_health = move |state: HealthState| {
    let _ = proxy_for_retry.send_event(UserEvent::HealthChanged(state));
};

// Spawn retry task (after pusher + print_worker spawns)
let retry_db_path = db_path.clone();
let retry_http = http.clone();
let retry_token = agent_token.clone();      // agent_token already cloned for worker_token
let retry_base_url = worker_base_url.clone();
rt_handle.spawn(async move {
    run_retry_task(
        retry_db_path,
        retry_token,
        retry_base_url,
        retry_http,
        printer_for_retry,   // second Box<dyn Printer> constructed from printer_id_clone
        retry_send_health,
    ).await;
});
```

[VERIFIED: pattern mirrors existing pusher proxy creation in main.rs lines 446-456]

### Pattern 4: toast call (D-07, #[cfg(windows)])

```rust
// In retry_task.rs — wrap in cfg gate (toast crate is Windows-only dep)
fn show_print_failure_toast() {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;
        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title("Brevly Print — Falha na impressão")
            .text1("Falha ao imprimir após 3 tentativas.")
            .text2("Verifique se a impressora está ligada e com papel.")
            .show();
    }
    #[cfg(not(windows))]
    eprintln!("[brevly-print] Retry task: print failure toast (Linux: stderr only)");
}
```

[VERIFIED: pattern matches existing `#[cfg(windows)]` usage for toast in CLAUDE.md stack research and `[target.'cfg(windows)'.dependencies]` in Cargo.toml]

### Pattern 5: fetch_pending_jobs() in noren_client.rs (D-08/D-09)

```rust
/// Fetches non-acked jobs from Noren for offline recovery (RES-03).
/// GET /api/agent/jobs/pending — returns jobs sorted by createdAt ASC, max 100.
pub async fn fetch_pending_jobs(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
) -> anyhow::Result<Vec<PendingJob>> {
    let url = format!("{base_url}/api/agent/jobs/pending");
    let resp = client
        .get(&url)
        .bearer_auth(agent_token)
        .send()
        .await
        .context("fetch_pending_jobs: HTTP transport error")?;

    match resp.status().as_u16() {
        200 => resp.json::<Vec<PendingJob>>()
            .await
            .context("fetch_pending_jobs: response parse error"),
        status => anyhow::bail!("fetch_pending_jobs: unexpected status {status}"),
    }
}

/// A non-acked job returned by GET /api/agent/jobs/pending.
///
/// Field names depend on the Noren contract (D-09):
/// - If Noren returns camelCase: use `#[serde(rename_all = "camelCase")]`
/// - If snake_case: no annotation needed.
/// Planner must confirm by grepping the Noren repo.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]   // ASSUMED — Noren is a JS/TS backend
pub struct PendingJob {
    pub job_id: String,
    pub job_type: String,
}
```

[ASSUMED for field names — D-09 explicitly defers to Noren contract]

### Anti-Patterns to Avoid

- **Shared printer between tasks via Mutex:** Each `Box<dyn Printer>` opens its own Win32 handle on every `print_raw()` call. Two concurrent calls are safe at the OS level. Using `Arc<Mutex<Box<dyn Printer>>>` would create false contention and add complexity with no benefit. The print worker and retry task each hold their own `Box<dyn Printer>`.

- **Calling `send_health` from `run_print_worker`:** The print worker only owns the happy path and the `retry_queue` insert. Health state transitions for failure scenarios are exclusively owned by the retry task (D-11). Adding `send_health` to `run_print_worker` would split health ownership across two tasks.

- **Re-fetching bytes during retry if already in retry_queue:** The `retry_queue.escpos_bytes` BLOB stores the bytes fetched at initial print time. The retry task MUST use these stored bytes, not call `fetch_job_bytes()` again. This avoids an additional network call per retry and handles the case where bytes are temporarily unavailable. Only crash recovery (D-05, row in `printed_jobs` but NOT in `retry_queue`) calls `fetch_job_bytes()`.

- **`tokio::time::sleep` loop instead of `interval`:** `interval` with `MissedTickBehavior::Delay` gives predictable 5s cadence even when a poll tick takes longer than 5s (e.g., slow print). A `sleep` loop would drift the effective interval under load.

- **Acking before `status='printed'` on retry success:** The C4 constraint applies to the retry task too. Retry success path: `UPDATE status='printed'` → `ack_job()` → `DELETE FROM retry_queue`. Never ack before the status update.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SQLite migration with new CHECK constraint | Manual ALTER TABLE / DROP-recreate logic from scratch | `rusqlite_migration` second `M::up` element | Migration versioning, idempotency, and rollback are handled; the table-recreation workaround is already the documented rusqlite_migration pattern |
| Periodic timer with missed-tick safety | Manual `tokio::time::sleep` + drift correction | `tokio::time::interval` with `MissedTickBehavior::Delay` | Already proven in `pusher/client.rs` ping timer (same pattern) |
| Windows toast | Win32/WinRT raw API (~30 LoC) | `tauri-winrt-notification` (already in Cargo.toml) | Already a project dependency from Phase 3; zero new code to add |
| HTTP GET with auth | Custom request builder | `reqwest` with `.bearer_auth()` | Already the established pattern in `noren_client.rs` |

**Key insight:** Phase 6 adds behavior to an already-established infrastructure. The primary intellectual work is correct ordering of SQL operations around crash fences and retry state transitions — not selecting new libraries.

---

## Common Pitfalls

### Pitfall 1: Migration v2 CHECK Constraint Syntax

**What goes wrong:** Writing the `M::up` for migration v2 without including all prior valid statuses in the new CHECK constraint causes existing rows to violate the constraint on INSERT/UPDATE and fail silently (or loudly with SQLITE_CONSTRAINT).

**Why it happens:** `CHECK(status IN ('pending','printing','printed','failed'))` must include all four values. Forgetting `'printed'` would break the print worker's success path.

**How to avoid:** The exact SQL is specified in D-01. Copy it verbatim. Verify with a test that round-trips through each status value.

**Warning signs:** Test asserting `UPDATE status='printed'` fails with `CHECK constraint failed: printed_jobs`.

### Pitfall 2: Double-write contention on printed_jobs

**What goes wrong:** Both `run_print_worker` and `run_retry_task` write to `printed_jobs`. A naive assumption that WAL handles this transparently without retry is wrong — WAL serializes writers, meaning one will briefly block if both try to write simultaneously.

**Why it happens:** SQLite WAL allows one writer at a time. A second writer is not rejected — it retries internally (busy timeout). If `busy_timeout` is not set, the second writer may return `SQLITE_BUSY` immediately.

**How to avoid:** Set `PRAGMA busy_timeout = 5000` (5 seconds) on each connection after enabling WAL. The existing connections in this codebase do NOT set busy_timeout — add it to all four connections in Phase 6. [ASSUMED: existing code does not set this; check at implementation time]

**Warning signs:** `database is locked` errors in logs under concurrent print + retry.

### Pitfall 3: Crash Recovery Infinite Loop

**What goes wrong:** If `fetch_job_bytes()` consistently fails for a `status='printing'` row that has no `retry_queue` entry, the retry task will attempt to re-fetch it on every boot, never clearing the row.

**Why it happens:** The D-05 crash recovery path logs and skips on `fetch_job_bytes()` failure, leaving the row at `status='printing'`. The query runs again on next boot.

**How to avoid:** This is acceptable behavior per D-05 ("idempotent startup check"). The design assumes `fetch_job_bytes()` failure is transient. Document this explicitly in the code comment. If a row is permanently unfetchable (Noren deleted it), it will loop forever — this is an explicit tradeoff for simplicity.

**Warning signs:** Same `job_id` appears in every boot log as "crash recovery fetch failed".

### Pitfall 4: printer_from_entry() requires a second call in main.rs

**What goes wrong:** The print worker's `printer_id` is constructed inside `run_print_worker`. For the retry task's separate `Box<dyn Printer>`, main.rs must read `printer_name` and `printer_type` from ConfigStore a SECOND time to construct a second `PrinterId` and call `printer_from_entry()` again.

**Why it happens:** `run_print_worker` consumes its printer. The retry task needs its own. Both must be constructed before their respective tasks are spawned.

**How to avoid:** In main.rs, before spawning tasks, read config once and call `printer_from_entry()` twice — once for the print worker (passed into `run_print_worker`), once for the retry task. Alternatively, store `PrinterId` as a `Clone`-able value and call `printer_from_entry()` once per task.

Note: `run_print_worker` currently reads `printer_name` and `printer_type` internally from the connection. Phase 6 changes must align: either move printer construction to main.rs for both tasks, or keep the pattern consistent (each task reads from its own connection).

**Warning signs:** Compiler error "use of moved value" when trying to share a printer between tasks.

### Pitfall 5: `tauri-winrt-notification` requires app registration on Windows 10/11

**What goes wrong:** Toast notifications may fail silently on Windows 10 if the app ID is not registered in the registry. `Toast::POWERSHELL_APP_ID` bypasses this requirement by using PowerShell's existing registration.

**Why it happens:** WinRT Toast API requires the calling process to have a registered AUMID (App User Model ID). `POWERSHELL_APP_ID` is a well-known ID that exists on all modern Windows installs.

**How to avoid:** Use `Toast::POWERSHELL_APP_ID` as specified in D-07. Check Phase 3 implementation for any custom app_id that was registered — if one exists, use it for consistency (the toast will show the correct app name). If Phase 3 used `POWERSHELL_APP_ID`, use it here too.

**Warning signs:** Toast `show()` returns `Err` — wrap in `let _ =` as specified (fire-and-forget).

### Pitfall 6: health_state.rs test will fail after D-10

**What goes wrong:** `health_state.rs` has a test `all_states_have_distinct_tooltips` that checks all three tooltips are distinct. If D-10 changes the `Problem` tooltip, the test still passes (all still distinct). But any test that asserts the exact string value of `Problem.tooltip()` would fail.

**Why it happens:** No existing test currently asserts exact string values (the test only checks distinctness). But a future test for retry exhaustion that also checks the tray string could fail if D-10 is not applied.

**How to avoid:** Apply D-10 first. Verify existing tests still pass after the string change. [VERIFIED: existing health_state tests only check distinctness, not exact values]

### Pitfall 7: Migration test must update expected user_version

**What goes wrong:** `config_store_test.rs` may assert `PRAGMA user_version = 1`. After adding migration v2, the version becomes 2. The test would fail.

**Why it happens:** `rusqlite_migration` advances `user_version` by one per `M::up`. Two migrations → `user_version = 2`.

**How to avoid:** After adding migration v2, update the `config_store_test.rs` assertion from `user_version = 1` to `user_version = 2`. [ASSUMED: check actual test; the fix is mechanical]

---

## Code Examples

### Migration v2 — config_store.rs (D-01)

```rust
// Source: 06-CONTEXT.md D-01 (locked decision)
static MIGRATIONS: LazyLock<Migrations<'static>> = LazyLock::new(|| {
    Migrations::new(vec![
        // v1 — creates config, printed_jobs (+ status index), retry_queue
        M::up(
            "CREATE TABLE config ( ... );
            CREATE TABLE printed_jobs (
                ...
                CHECK(status IN ('pending','printed','failed'))
            );
            ...",
        ),
        // v2 — add 'printing' intermediate status for crash recovery (RES-04)
        M::up(
            "CREATE TABLE printed_jobs_v2 (
                job_id      TEXT PRIMARY KEY NOT NULL,
                job_type    TEXT,
                status      TEXT NOT NULL DEFAULT 'pending'
                                CHECK(status IN ('pending','printing','printed','failed')),
                attempt     INTEGER NOT NULL DEFAULT 0,
                received_at TEXT,
                printed_at  TEXT,
                failed_at   TEXT
            );
            INSERT INTO printed_jobs_v2 SELECT * FROM printed_jobs;
            DROP TABLE printed_jobs;
            ALTER TABLE printed_jobs_v2 RENAME TO printed_jobs;
            CREATE INDEX idx_printed_jobs_status ON printed_jobs(status);",
        ),
    ])
});
```

Note: the v1 migration also creates `idx_printed_jobs_status`. After table recreation in v2, the index is dropped (with the table) and re-created. This is correct — the v2 `M::up` creates a fresh index on the renamed table.

### Crash recovery startup query (D-05)

```rust
// Source: 06-CONTEXT.md D-05 (locked decision)
// Rows left at 'printing' that never made it into retry_queue
// (crash happened between print_worker writing 'printing' and inserting retry_queue row).
let mut stmt = conn.prepare(
    "SELECT job_id, job_type FROM printed_jobs
     WHERE status = 'printing'
       AND job_id NOT IN (SELECT job_id FROM retry_queue)"
)?;
```

### retry_queue poll query (D-06)

```rust
// Source: 06-CONTEXT.md D-06 (locked decision)
let mut stmt = conn.prepare(
    "SELECT job_id, job_type, escpos_bytes, attempt_count
     FROM retry_queue
     WHERE next_retry_at <= datetime('now')
     ORDER BY next_retry_at ASC
     LIMIT 10"
)?;
```

### Pending pull insertion (D-08 — in pusher/client.rs)

```rust
// After: send_health(HealthState::Connected); + attempt = 0;
// Source: 06-CONTEXT.md D-08 (locked decision)

// RES-03: pull pending jobs on every successful Pusher reconnect.
// Error handling: log + continue (best-effort; next reconnect retries).
match noren_client::fetch_pending_jobs(&http, &base_url, &agent_token).await {
    Ok(pending) => {
        for job in pending {
            match insert_print_job(&pusher_conn, &job.job_id, &job.job_type) {
                Ok(true) => {
                    // New job — forward to print worker (same WR-04 try_send pattern)
                    match tx.try_send(PrintEvent { job_id: job.job_id.clone(), job_type: job.job_type }) {
                        Ok(()) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Full(ev)) => {
                            eprintln!("[brevly-print] Pusher: pending pull channel full — job {} queued via background send", ev.job_id);
                            let tx2 = tx.clone();
                            tokio::spawn(async move { let _ = tx2.send(ev).await; });
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                            eprintln!("[brevly-print] Pusher: print channel closed during pending pull");
                            break 'inner true;
                        }
                    }
                }
                Ok(false) => {} // already printed — dedup fence (C3) no-ops
                Err(e) => eprintln!("[brevly-print] Pusher: pending pull SQLite insert failed: {e:#}"),
            }
        }
    }
    Err(e) => {
        eprintln!("[brevly-print] Pusher: pending pull failed — {e:#} (will retry on next reconnect)");
    }
}
```

---

## State of the Art

| Old Approach | Current Approach | Impact |
|--------------|------------------|--------|
| `retry_queue` was defined in schema v1 but never written | Phase 6 `print_worker.rs` writes to it on failure; `retry_task.rs` reads it | Existing table structure requires no schema change beyond `printed_jobs` migration |
| `status` values were `('pending','printed','failed')` — no in-flight marker | v2 adds `'printing'` as crash recovery fence | Jobs can never be silently lost across process crashes |

**Deprecated/outdated:**

- Phase 5's comment `"leave status='pending' — Phase 6 retries"` in `print_worker.rs` (lines 133, 144) will become outdated. Phase 6 changes those paths to: set `status='printing'` before print → on failure, insert into `retry_queue`. The old comment was a correct placeholder that must be removed/replaced.

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `PendingJob` fields are camelCase (`jobId`, `jobType`) — Noren is a JS/TS backend | Pattern 5, fetch_pending_jobs | Low — `#[serde(rename_all = "camelCase")]` either works or the JSON parse fails visibly; planner must confirm from Noren repo (D-09) |
| A2 | `PRAGMA busy_timeout` is not currently set on existing connections | Pitfall 2 | Low — if it is set, the pitfall is already mitigated; if not, Phase 6 should add it to all 4 connections |
| A3 | `config_store_test.rs` asserts `user_version = 1` | Pitfall 7 | Low — test will fail at CI if wrong; easy to fix |
| A4 | `run_print_worker` currently constructs its printer internally (reading printer_name from its own conn); Phase 6 must either refactor to pass printer from main.rs or keep the pattern per-task | Pitfall 4 | Medium — if not addressed, the retry task has no clean path to construct a second printer without reading the same config keys again |
| A5 | Phase 3 used `Toast::POWERSHELL_APP_ID` (not a custom registered app_id) | Pattern 4 | Low — the existing tray_runtime.rs or Phase 3 toast implementation should be checked; using POWERSHELL_APP_ID is the safe fallback |

---

## Open Questions

1. **Noren `PendingJob` field name contract (D-09)**
   - What we know: Noren is a SvelteKit/TypeScript backend; camelCase JSON output is standard JS convention.
   - What's unclear: The exact JSON shape of `GET /api/agent/jobs/pending` — field names `jobId`/`type` vs `job_id`/`job_type` vs some other convention.
   - Recommendation: Planner must grep `~/repos/brevly/noren` for the pending jobs route handler before writing `PendingJob`. If the Noren repo is not available locally, default to `#[serde(rename_all = "camelCase")]` with `job_id`/`job_type` field names.

2. **printer construction in main.rs for retry task (A4)**
   - What we know: `run_print_worker` currently reads `printer_name` and `printer_type` from its own SQLite connection inside the function. The retry task needs a separate `Box<dyn Printer>`.
   - What's unclear: Whether Phase 6 should refactor `run_print_worker` to accept a `Box<dyn Printer>` parameter (breaking change to signature) or leave it as-is and construct the retry printer separately in main.rs by re-reading the same config keys.
   - Recommendation: The planner should choose the approach that avoids duplicate config reads AND does not break existing tests. Passing `Box<dyn Printer>` as a parameter to both tasks (constructed once in main.rs) is cleaner but requires a signature change. Keeping the internal construction in `run_print_worker` and duplicating the printer_id construction for the retry task is simpler. Both are valid — discretion area per CONTEXT.md.

3. **busy_timeout on SQLite connections**
   - What we know: WAL serializes writers. Simultaneous writes from print_worker and retry_task on `printed_jobs` will briefly contend.
   - What's unclear: Whether `SQLITE_BUSY` with default timeout (0ms) could cause intermittent write failures under realistic load (concurrent print + retry).
   - Recommendation: Add `conn.busy_timeout(Duration::from_secs(5))` after WAL enable on all connections in Phase 6. At most 10 rows process per 5s poll cycle; contention windows are microseconds. A 5s timeout is conservative.

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `cargo` / Rust toolchain | Build | ✓ | (project compiles) | — |
| `rusqlite` bundled SQLite | Migration v2, retry_queue | ✓ | 0.40 in Cargo.toml | — |
| `tokio` | retry_task interval | ✓ | 1.x in Cargo.toml | — |
| `tauri-winrt-notification` | RES-02 toast | ✓ (Windows-only dep) | 0.8 in Cargo.toml | Linux: stderr log |
| `reqwest` | fetch_pending_jobs | ✓ | 0.13 in Cargo.toml | — |
| Noren `GET /api/agent/jobs/pending` | RES-03 | ✗ (not yet live) | — | Mock HTTP stub in tests (same `spawn_stub` pattern as noren_client_test.rs) |

**Missing dependencies with no fallback:** None — all Rust dependencies are present.

**Missing dependencies with fallback:** Noren `GET /api/agent/jobs/pending` endpoint. RES-03 can be fully integration-tested using the `spawn_stub` pattern already established in `tests/noren_client_test.rs`. The retry path (RES-01/02/04) is fully testable without Noren using an in-process mock printer that always fails.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness (`cargo test`) |
| Config file | none — `cargo test` discovers tests automatically |
| Quick run command | `cargo test` |
| Full suite command | `cargo test` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| RES-01 | Retry 3× at 30s intervals | unit | `cargo test retry_task` | ❌ Wave 0: `tests/retry_task_test.rs` |
| RES-01 | retry_queue INSERT on print_worker failure | unit | `cargo test retry_queue_insert` | ❌ Wave 0 |
| RES-02 | Toast called after attempt_count >= 3 | unit (logic only) | `cargo test retry_exhaustion` | ❌ Wave 0 |
| RES-02 | send_health(Problem) called on exhaustion | unit | `cargo test retry_exhaustion` | ❌ Wave 0 |
| RES-03 | fetch_pending_jobs 200 returns Vec<PendingJob> | unit (HTTP stub) | `cargo test fetch_pending_jobs` | ❌ Wave 0 |
| RES-03 | fetch_pending_jobs failure logs and continues | unit | `cargo test fetch_pending_jobs_error` | ❌ Wave 0 |
| RES-03 | Dedup: duplicate job_id in pending pull is no-op | unit | existing `insert_print_job_returns_false_on_duplicate` in `pusher/client.rs` | ✅ |
| RES-04 | Crash recovery: `status='printing'` rows with no retry_queue entry are inserted | unit (SQLite in-memory) | `cargo test crash_recovery` | ❌ Wave 0 |
| RES-04 | Crash recovery: rows already in retry_queue are NOT re-inserted | unit | `cargo test crash_recovery_already_queued` | ❌ Wave 0 |
| D-10 | Problem tooltip/label updated | unit | `cargo test health_state` (existing, update expected strings) | ✅ (update needed) |
| Migration | user_version advances to 2 after v2 migration | unit | `cargo test migration` (update expected version) | ✅ (update needed) |

### Sampling Rate

- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `tests/retry_task_test.rs` — covers RES-01 (retry queue logic), RES-02 (exhaustion path), RES-04 (crash recovery query)
- [ ] Add `fetch_pending_jobs` tests to `tests/noren_client_test.rs` (or new `pending_jobs_test.rs`) — covers RES-03
- [ ] Update `tests/config_store_test.rs` assertion for `user_version` from 1 → 2 after migration v2
- [ ] Update `health_state.rs` tests if any assert exact `Problem` strings (currently only distinctness is checked — no update needed, but verify)

---

## Security Domain

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No (no new auth flow) | — |
| V3 Session Management | No | — |
| V4 Access Control | No | — |
| V5 Input Validation | Yes — `fetch_pending_jobs` returns job_id values that flow into `validate_job_id()` before URL construction | `validate_job_id()` already in `noren_client.rs` (CR-02 — path traversal guard) |
| V6 Cryptography | No | — |

### Known Threat Patterns for Phase 6 stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Path traversal via `job_id` from `/api/agent/jobs/pending` | Tampering | `validate_job_id()` in `noren_client.rs` — already rejects `/`, `.`, `\`, `?`, `#`, `%`, NUL |
| Token logging in error messages | Information Disclosure | `agent_token` passed only via `.bearer_auth()` — never in `eprintln!` (T-02-02; enforce in `fetch_pending_jobs()`) |
| `escpos_bytes` BLOB content | Tampering | Agent is a spooler — it prints whatever Noren sends. Trust boundary is the Noren backend authentication; agent does not validate ESC/POS content |

**Critical:** `fetch_pending_jobs()` must call `validate_job_id()` on each `job_id` before forwarding it to `insert_print_job()` or constructing any URL. The pending pull is a new code path where job_ids arrive from the network — the same path-traversal risk as Phase 5's `fetch_job_bytes()` applies here.

---

## Sources

### Primary (HIGH confidence — verified from actual source files)

- `src/config_store.rs` — migration v1 structure, `MIGRATIONS` vec, `rusqlite_migration` usage
- `src/print_worker.rs` — current print pipeline, failure paths, `retry_queue` insert point (lines 139-144)
- `src/pusher/client.rs` — `subscription_succeeded` handling at line ~289, `insert_print_job()` function, `try_send` WR-04 pattern
- `src/health_state.rs` — current `Problem` string values, existing tests
- `src/noren_client.rs` — `fetch_job_bytes()` / `ack_job()` / `pusher_auth()` patterns, `validate_job_id()` (CR-02)
- `src/printer/mod.rs` — `Printer` trait with `Send` superbound, `printer_from_entry()`
- `src/main.rs` — Runtime block spawn pattern (lines 403-466), `create_proxy()` / health closure pattern
- `src/lib.rs` — module declarations to update
- `Cargo.toml` — confirms `tauri-winrt-notification = "0.8"` already present; no new deps needed
- `.planning/phases/06-resilience/06-CONTEXT.md` — all locked decisions D-01 through D-12

### Secondary (MEDIUM confidence — from CONTEXT.md canonical references)

- `06-CONTEXT.md §canonical_refs` → Phase 5 decisions carried forward (D-08, D-09, WR-02)
- `06-CONTEXT.md §specifics` → BLOB binding: `rusqlite::params![..., bytes.as_slice(), ...]`

### Tertiary (LOW / ASSUMED — requires confirmation at implementation time)

- Noren `PendingJob` JSON field names — D-09 explicitly defers; assumed camelCase
- `PRAGMA busy_timeout` not set on existing connections — assumed, not verified by reading all connection setup code exhaustively

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — no new dependencies; all confirmed in Cargo.toml
- Architecture: HIGH — retry task pattern mirrors existing Pusher/print_worker spawn pattern exactly
- Migration: HIGH — table-recreation workaround is the standard SQLite pattern; `rusqlite_migration` multi-statement `M::up` already proven in v1
- Pitfalls: MEDIUM-HIGH — most derived from reading actual code; Pitfall 2 (busy_timeout) is ASSUMED
- Pending pull (RES-03): MEDIUM — implementation side is clear; Noren field name contract is TBD (D-09)

**Research date:** 2026-07-16
**Valid until:** 2026-08-16 (stable Rust ecosystem; Noren contract confirmation is the only dynamic element)
