# Phase 6: Resilience - Pattern Map

**Mapped:** 2026-07-16
**Files analyzed:** 8 (1 new, 7 modified)
**Analogs found:** 8 / 8

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/retry_task.rs` | service (async task) | event-driven + CRUD | `src/print_worker.rs` | exact — same task shape: open WAL conn, startup block, poll loop, printer call, health closure |
| `src/print_worker.rs` | service (async task) | event-driven + CRUD | self | — (modification target) |
| `src/config_store.rs` | config + migration | batch | self | — (modification target) |
| `src/noren_client.rs` | service (HTTP client) | request-response | self (`fetch_job_bytes`, `ack_job`) | exact — new function follows established pattern in same file |
| `src/pusher/client.rs` | service (WebSocket loop) | event-driven | self | — (modification target) |
| `src/health_state.rs` | utility (enum + strings) | — | self | — (string-only modification target) |
| `src/lib.rs` | config (module declarations) | — | self | — (one-line addition) |
| `src/main.rs` | config (task spawning) | — | self (lines 454–465) | exact — retry task spawn mirrors pusher + print_worker spawn |
| `tests/retry_task_test.rs` | test | — | `src/pusher/client.rs` (tests block, lines 427–533) | role-match — same in-memory SQLite test pattern |

---

## Pattern Assignments

### `src/retry_task.rs` (new file — async task, event-driven + CRUD)

**Primary analog:** `src/print_worker.rs` (lines 1–180)
**Secondary analog for health closure:** `src/pusher/client.rs` (lines 131–138 for signature, 446–456 in main.rs for proxy creation)

**Imports pattern** (copy from `src/print_worker.rs` lines 10–19, augmented):
```rust
use std::path::PathBuf;
use std::time::Duration;

use tokio::time::{interval, MissedTickBehavior};

use crate::{
    health_state::HealthState,
    noren_client::{ack_job, fetch_job_bytes, validate_job_id},
    printer::{printer_from_entry, PrinterId},
};
```

**WAL connection open pattern** (copy from `src/print_worker.rs` lines 36–51 verbatim):
```rust
// ── Startup: open a FOURTH SQLite connection (D-04) ─────────────────────────
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

**Function signature pattern** (D-03 — drives `send_health` closure type from `src/pusher/client.rs` lines 134–138):
```rust
pub async fn run_retry_task(
    db_path: PathBuf,
    agent_token: String,
    base_url: String,
    http: reqwest::Client,
    printer: Box<dyn crate::printer::Printer + Send>,
    send_health: impl Fn(HealthState) + Send + 'static,
) {
    // ... WAL open ...
```

**Poll loop interval pattern** (copy from `src/pusher/client.rs` lines 295–300 — same `MissedTickBehavior::Delay`):
```rust
let mut poll_timer = interval(Duration::from_secs(5));
poll_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);
poll_timer.tick().await; // burn first immediate tick
```

**Crash recovery startup query** (D-05 — concrete SQL with `conn.prepare` pattern from `src/pusher/client.rs` `insert_print_job`):
```rust
// D-05: reprocess jobs that crashed between status='printing' and retry_queue INSERT
let mut stmt = conn.prepare(
    "SELECT job_id, job_type FROM printed_jobs
     WHERE status = 'printing'
       AND job_id NOT IN (SELECT job_id FROM retry_queue)"
)?;
let orphans: Vec<(String, String)> = stmt
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
    .filter_map(|r| r.ok())
    .collect();
for (job_id, job_type) in orphans {
    match fetch_job_bytes(&http, &base_url, &agent_token, &job_id).await {
        Ok(bytes) => {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO retry_queue
                    (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
                 VALUES (?1, ?2, ?3, 1, datetime('now'), 'crash recovery', datetime('now'))",
                rusqlite::params![job_id, job_type, bytes.as_slice()],
            );
        }
        Err(e) => {
            eprintln!("[brevly-print] Retry task: crash recovery fetch failed for {job_id}: {e:#}");
            // leave status='printing' — retry on next boot (idempotent)
        }
    }
}
```

**Retry poll query and success/failure branching** (D-06 — `conn.execute` pattern from `src/print_worker.rs` lines 151–167, `ack_job` from `src/noren_client.rs` lines 306–328):
```rust
// Poll loop body — runs every 5s
let mut stmt = conn.prepare(
    "SELECT job_id, job_type, escpos_bytes, attempt_count
     FROM retry_queue
     WHERE next_retry_at <= datetime('now')
     ORDER BY next_retry_at ASC
     LIMIT 10"
)?;
let rows: Vec<(String, String, Vec<u8>, i64)> = stmt
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
    .filter_map(|r| r.ok())
    .collect();

for (job_id, job_type, escpos_bytes, attempt_count) in rows {
    // Crash fence (same pattern as D-02 in print_worker):
    let _ = conn.execute(
        "UPDATE printed_jobs SET status='printing' WHERE job_id=?1",
        rusqlite::params![job_id],
    );

    match printer.print_raw(&escpos_bytes) {
        Ok(()) => {
            // C4 ordering: UPDATE status='printed' BEFORE ack_job
            let _ = conn.execute(
                "UPDATE printed_jobs SET status='printed', printed_at=datetime('now') WHERE job_id=?1",
                rusqlite::params![job_id],
            );
            if let Err(e) = ack_job(&http, &base_url, &agent_token, &job_id).await {
                eprintln!("[brevly-print] Retry task: ack failed for {job_id}: {e:#}");
            }
            let _ = conn.execute(
                "DELETE FROM retry_queue WHERE job_id=?1",
                rusqlite::params![job_id],
            );
            send_health(HealthState::Connected);
        }
        Err(e) if attempt_count < 3 => {
            let msg = e.to_string();
            let _ = conn.execute(
                "UPDATE retry_queue SET attempt_count=attempt_count+1,
                     next_retry_at=datetime('now', '+30 seconds'), last_error=?2
                 WHERE job_id=?1",
                rusqlite::params![job_id, msg],
            );
        }
        Err(e) => {
            // attempt_count >= 3: exhaust job
            eprintln!("[brevly-print] Retry task: job {job_id} exhausted after 3 attempts: {e:#}");
            let _ = conn.execute(
                "UPDATE printed_jobs SET status='failed', failed_at=datetime('now') WHERE job_id=?1",
                rusqlite::params![job_id],
            );
            let _ = conn.execute(
                "DELETE FROM retry_queue WHERE job_id=?1",
                rusqlite::params![job_id],
            );
            send_health(HealthState::Problem);
            show_print_failure_toast();
        }
    }
}
```

**Toast helper pattern** (D-07 — `#[cfg(windows)]` gate, same fire-and-forget `let _ =` pattern as existing toast usage in the project):
```rust
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

---

### `src/print_worker.rs` (modified — two surgical insertions)

**Analog:** self — read before writing; insertion points identified below.

**D-02 insertion point — `status='printing'` fence** (insert between lines 134 and 136, after `fetch_job_bytes` success, before `printer.print_raw`):

Copy the `conn.execute` pattern from lines 151–167 (the `status='printed'` UPDATE), but use `status='printing'` with no timestamp fields:
```rust
// D-02: set crash-recovery fence BEFORE calling print_raw (RES-04).
// If the process crashes here, the row stays at 'printing' and the retry task
// re-queues it at startup (D-05). Leave status='printing' on failure too —
// the retry task owns the transition to 'failed'.
match conn.execute(
    "UPDATE printed_jobs SET status='printing', attempt=attempt+1 WHERE job_id=?1",
    rusqlite::params![event.job_id],
) {
    Ok(0) => eprintln!(
        "[brevly-print] Print worker: UPDATE to 'printing' matched 0 rows for {} — row absent",
        event.job_id
    ),
    Ok(_) => {}
    Err(e) => eprintln!(
        "[brevly-print] Print worker: SQLite update to 'printing' failed for {}: {e:#}",
        event.job_id
    ),
}
```

**D-12 insertion point — `retry_queue` INSERT on failure** (replace `continue` at line 144 with INSERT, then continue):

Copy BLOB binding pattern from RESEARCH.md Pattern 2 — `bytes.as_slice()`:
```rust
// D-12: on print failure, insert into retry_queue for Phase 6 retry task.
// INSERT OR IGNORE prevents double-insert if the same job_id arrives twice.
// status stays 'printing' (intentional — retry task uses retry_queue.escpos_bytes).
let error_msg = e.to_string();
if let Err(db_err) = conn.execute(
    "INSERT OR IGNORE INTO retry_queue
         (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
     VALUES
         (?1, ?2, ?3, 1, datetime('now', '+30 seconds'), ?4, datetime('now'))",
    rusqlite::params![event.job_id, event.job_type, bytes.as_slice(), error_msg],
) {
    eprintln!(
        "[brevly-print] Print worker: retry_queue INSERT failed for {}: {db_err:#}",
        event.job_id
    );
}
// Do NOT update attempt counter in retry_queue — that is the retry task's job (D-06).
continue;
```

---

### `src/config_store.rs` (modified — add migration v2 to `MIGRATIONS` vec)

**Analog:** self — read lines 30–61. The second `M::up` is appended to the vec inside `MIGRATIONS`.

**Existing v1 pattern** (lines 30–61) — match this style exactly:
```rust
static MIGRATIONS: LazyLock<Migrations<'static>> = LazyLock::new(|| {
    Migrations::new(vec![
        // v1 — (existing, unchanged)
        M::up("..."),
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

Note: the v1 migration already creates `idx_printed_jobs_status`. After table recreation, the index is dropped with the table and re-created by v2. This is correct — v2 creates a fresh index on the renamed table.

---

### `src/noren_client.rs` (modified — add `fetch_pending_jobs` + `PendingJob`)

**Analog:** `fetch_job_bytes` (lines 257–295) and `ack_job` (lines 306–328) in the same file.

**`PendingJob` struct** — copy `ActivateResponse` struct pattern (lines 53–61): uses `#[derive(Deserialize)]` + `#[serde(rename_all = "camelCase")]`:
```rust
/// A non-acked job returned by GET /api/agent/jobs/pending.
///
/// Field names: Noren is a JS/TS backend — camelCase JSON expected (D-09).
/// Planner must confirm from the Noren repo before finalising `rename_all`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingJob {
    pub job_id: String,
    pub job_type: String,
}
```

**`fetch_pending_jobs` function** — copy `fetch_job_bytes` (lines 257–295) bearer auth + anyhow::Result + status match pattern:
```rust
/// Fetches non-acked jobs from Noren for offline recovery (RES-03).
/// GET /api/agent/jobs/pending — returns jobs sorted by createdAt ASC, max 100.
///
/// `agent_token` passed via `.bearer_auth()` only — never in any log (T-02-02).
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
        200 => resp
            .json::<Vec<PendingJob>>()
            .await
            .context("fetch_pending_jobs: response parse error"),
        status => anyhow::bail!("fetch_pending_jobs: unexpected status {status}"),
    }
}
```

Note: `validate_job_id` (lines 168–176) must be called on each `job_id` from the response before passing to `insert_print_job` or URL construction (CR-02 / security section of RESEARCH.md).

---

### `src/pusher/client.rs` (modified — add pending pull after `subscription_succeeded`)

**Analog:** self — existing `print:job` handling block (lines 324–372) for the `try_send` WR-04 pattern.

**Insertion point:** lines 289–292 (after `send_health(HealthState::Connected)` / `attempt = 0`).

**Pending pull pattern** (D-08 — copy WR-04 `try_send` + spawn-on-Full pattern from lines 335–355):
```rust
// RES-03: pull pending jobs on every successful Pusher reconnect (D-08).
// Error handling: log + continue — do NOT reconnect WebSocket on failure.
match noren_client::fetch_pending_jobs(&http, &base_url, &agent_token).await {
    Ok(pending) => {
        for job in pending {
            // Security: validate job_id before INSERT (CR-02).
            if let Err(e) = noren_client::validate_job_id(&job.job_id) {
                eprintln!("[brevly-print] Pusher: pending pull invalid job_id: {e:#}");
                continue;
            }
            match insert_print_job(&pusher_conn, &job.job_id, &job.job_type) {
                Ok(true) => {
                    // New job — forward to print worker (WR-04 try_send pattern)
                    match tx.try_send(PrintEvent { job_id: job.job_id.clone(), job_type: job.job_type }) {
                        Ok(()) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Full(ev)) => {
                            eprintln!(
                                "[brevly-print] Pusher: pending pull channel full — \
                                 job {} queued via background send",
                                ev.job_id
                            );
                            let tx2 = tx.clone();
                            tokio::spawn(async move { let _ = tx2.send(ev).await; });
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                            eprintln!(
                                "[brevly-print] Pusher: print channel closed during pending pull"
                            );
                            // break out of for loop and let the outer reconnect loop handle it
                            break;
                        }
                    }
                }
                Ok(false) => {} // already printed — dedup fence (C3) no-ops
                Err(e) => eprintln!(
                    "[brevly-print] Pusher: pending pull SQLite insert failed: {e:#}"
                ),
            }
        }
    }
    Err(e) => {
        eprintln!(
            "[brevly-print] Pusher: pending pull failed — {e:#} (will retry on next reconnect)"
        );
    }
}
```

Also add `noren_client` import if not already destructured at the use site (currently `noren_client::pusher_auth` is imported by name at line 40 — add `fetch_pending_jobs` to the use path or call via full path).

---

### `src/health_state.rs` (modified — two string literals only)

**D-10 changes** (lines 27 and 34 — exact strings):

| Location | Before | After |
|----------|--------|-------|
| `tooltip()` `Problem` arm (line 27) | `"Brevly Print — Problema de conexão"` | `"Brevly Print — Falha na impressora"` |
| `status_label()` `Problem` arm (line 34) | `"Problema de conexão"` | `"Falha na impressora"` |

Existing test `all_states_have_distinct_tooltips` (line 62) will still pass after D-10 — it checks distinctness only, not exact values. `status_labels_are_non_empty` (line 78) will also still pass.

---

### `src/lib.rs` (modified — one-line module declaration)

**Analog:** existing module declarations lines 8–17.

Add after line 17 (`pub mod print_worker;`):
```rust
pub mod retry_task;
```

---

### `src/main.rs` (modified — retry task spawn in Runtime block)

**Analog:** lines 444–465 (Pusher proxy creation + print_worker spawn). Copy the exact proxy creation + clone pattern.

**Retry task health proxy** (after line 448, before Pusher spawn at line 454):
```rust
// Health closure for retry task (D-03) — same pattern as send_health for Pusher.
let proxy_for_retry = event_loop.create_proxy();
let retry_send_health = move |state: HealthState| {
    let _ = proxy_for_retry.send_event(UserEvent::HealthChanged(state));
};
```

**Read printer config for retry task** (before spawning — same block as lines 63–84 of `print_worker.rs`, duplicated in main.rs):
```rust
// Second printer construction for retry task (D-03 / Pitfall 4).
// run_print_worker reads its own printer internally; retry task needs its own Box<dyn Printer>.
let retry_printer_name = config_store::get(&conn, "printer_name")
    .unwrap_or(None)
    .filter(|s| !s.is_empty())
    .unwrap_or_default();
let retry_printer_type = config_store::get(&conn, "printer_type")
    .unwrap_or(None)
    .unwrap_or_default();
let retry_printer_id = if retry_printer_type == "serial" {
    PrinterId::Serial(retry_printer_name)
} else {
    PrinterId::Spooler(retry_printer_name)
};
let printer_for_retry = printer_from_entry(&retry_printer_id);
```

**Retry task spawn** (after line 461 print_worker spawn, before line 465 `drop(print_tx)`):
```rust
// Phase 6: spawn retry task — fourth Tokio task (D-03).
let retry_db_path = db_path.clone();
let retry_http = http.clone();
let retry_token = worker_token.clone();       // already cloned from agent_token
let retry_base_url = worker_base_url.clone(); // already cloned from auth_url
rt_handle.spawn(async move {
    crate::retry_task::run_retry_task(
        retry_db_path,
        retry_token,
        retry_base_url,
        retry_http,
        printer_for_retry,
        retry_send_health,
    ).await;
});
```

---

### `tests/retry_task_test.rs` (new file — tests)

**Analog:** `src/pusher/client.rs` tests block (lines 427–533) — `make_test_conn()` pattern + `rusqlite::Connection::open_in_memory()`.

**In-memory conn helper pattern** (copy from `pusher/client.rs` lines 432–447, extend schema for both tables):
```rust
fn make_test_conn() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory DB");
    conn.execute_batch(
        "CREATE TABLE printed_jobs (
            job_id      TEXT PRIMARY KEY NOT NULL,
            job_type    TEXT,
            status      TEXT NOT NULL DEFAULT 'pending',
            attempt     INTEGER NOT NULL DEFAULT 0,
            received_at TEXT,
            printed_at  TEXT,
            failed_at   TEXT
        );
        CREATE TABLE retry_queue (
            job_id        TEXT PRIMARY KEY NOT NULL,
            job_type      TEXT,
            escpos_bytes  BLOB,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            next_retry_at TEXT,
            last_error    TEXT,
            created_at    TEXT
        );",
    )
    .expect("create test schema");
    conn
}
```

**Test structure** — unit tests using `#[test]` (not `#[tokio::test]`) where possible for SQL assertions; `#[tokio::test]` for async paths. Same pattern as `pusher/client.rs` tests.

---

## Shared Patterns

### WAL Connection Open
**Source:** `src/print_worker.rs` lines 36–51
**Apply to:** `src/retry_task.rs` startup
```rust
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
Note: also add `conn.busy_timeout(std::time::Duration::from_secs(5))?;` after WAL enable on all four connections (Pitfall 2 / RESEARCH.md). Existing connections in `print_worker.rs` and `pusher/client.rs` do not set this — add to all in Phase 6.

### Health Closure + EventLoopProxy
**Source:** `src/main.rs` lines 446–449, used by `src/pusher/client.rs` parameter at line 135
**Apply to:** `src/retry_task.rs` (same `impl Fn(HealthState) + Send + 'static` parameter type)
```rust
// In main.rs (creation):
let proxy_for_retry = event_loop.create_proxy();
let retry_send_health = move |state: HealthState| {
    let _ = proxy_for_retry.send_event(UserEvent::HealthChanged(state));
};

// In retry_task.rs (usage):
send_health(HealthState::Connected);  // on retry success
send_health(HealthState::Problem);    // on retry exhaustion
```

### `conn.execute` + `rusqlite::params!` Pattern
**Source:** `src/print_worker.rs` lines 95–113 (disabled-type UPDATE), lines 151–167 (success UPDATE)
**Apply to:** All `conn.execute` calls in `src/retry_task.rs` — use the same `match` arms: `Ok(0)` → eprintln, `Ok(_)` → continue, `Err(e)` → eprintln.

### `eprintln!` Logging Prefix
**Source:** All existing files — e.g. `src/pusher/client.rs` line 153: `"[brevly-print] Pusher task: ..."`
**Apply to:** `src/retry_task.rs` uses prefix `"[brevly-print] Retry task: ..."`; additions to `src/pusher/client.rs` use `"[brevly-print] Pusher: pending pull: ..."`.

### Bearer Auth HTTP Request
**Source:** `src/noren_client.rs` lines 276–281 (`fetch_job_bytes`) and lines 319–322 (`ack_job`)
**Apply to:** `fetch_pending_jobs` in `src/noren_client.rs` — same `.bearer_auth(agent_token)` pattern; token never in `eprintln!`.

### `validate_job_id` Before Network-Derived job_id
**Source:** `src/noren_client.rs` lines 168–176 + usage at lines 266 and 313
**Apply to:** `fetch_pending_jobs` callers — each `job.job_id` from the pending pull response must pass through `validate_job_id` before being forwarded (CR-02 — path traversal guard).

### `#[cfg(windows)]` Conditional Compilation for Toast
**Source:** Established in `Cargo.toml` `[target.'cfg(windows)'.dependencies]` for `tauri-winrt-notification`
**Apply to:** `show_print_failure_toast()` helper in `retry_task.rs` — wrap in `#[cfg(windows)]` block; `#[cfg(not(windows))]` arm logs to stderr.

---

## No Analog Found

None — all Phase 6 files have close analogs in the codebase.

---

## Critical Ordering Constraints (carry into plan)

These are not patterns but ordering invariants that the planner must encode as task ordering:

1. **Migration v2 must run at startup before any task touches `status='printing'`** — guaranteed by `open_and_migrate()` in `main.rs` before the Runtime block; no extra ordering needed.
2. **C4 applies to retry task too:** `UPDATE status='printed'` → `ack_job()` → `DELETE FROM retry_queue`. Never ack before status update.
3. **D-11:** `run_print_worker` does NOT get `send_health`. Health transitions on failure are exclusively the retry task's responsibility.
4. **Existing `tests/config_store_test.rs`** likely asserts `user_version = 1` — update to `user_version = 2` after migration v2 is added (Pitfall 7 / RESEARCH.md).
5. **`validate_job_id`** is currently a private function (`fn`, not `pub fn`) in `noren_client.rs` — it must be made `pub(crate)` for the Pusher client to call it on `job_id` values from `fetch_pending_jobs`.

---

## Metadata

**Analog search scope:** `src/` (all `.rs` files), `tests/` (test files)
**Files read:** `src/print_worker.rs`, `src/config_store.rs`, `src/noren_client.rs`, `src/pusher/client.rs`, `src/health_state.rs`, `src/printer/mod.rs`, `src/main.rs` (lines 1–100, 380–486), `src/lib.rs`
**Files scanned (structure):** all 25 `.rs` files in the project
**Pattern extraction date:** 2026-07-16
