# Phase 6: Resilience - Context

**Gathered:** 2026-07-16
**Status:** Ready for planning
**Note:** Owner delegated all decisions ("pode decidir tudo e já criar o contexto, não entendo
muito sobre a parte técnica") — same posture as Phases 1, 3, 4, and 5. Every decision below is
**locked** (Claude's discretion, exercised). Downstream agents should treat these as decided,
not open.

<domain>
## Phase Boundary

Make the agent resilient to the two failure modes that would otherwise cause lost tickets:

1. **Printer failure** — paper out, printer offline, USB disconnect. The print worker
   currently leaves a failed job at `status='pending'` and moves on; Phase 6 adds retries
   (3× / 30s) and, after exhaustion, a Windows toast notification + red tray icon.

2. **Internet outage** — while the agent is disconnected from Pusher, Noren queues jobs
   server-side. Phase 6 adds a pending pull (`GET /api/agent/jobs/pending`) triggered
   immediately after every successful Pusher reconnect, draining any missed jobs through
   the existing print pipeline.

3. **Crash recovery** — jobs left in `status='printing'` (the new intermediate fence) are
   reprocessed at startup so a process crash mid-print never silently loses a ticket.

**What Phase 6 owns (all 4 requirements):**
- `RES-01`: Retry 3× / 30s on printer failure
- `RES-02`: Toast + red tray after 3 failed retries
- `RES-03`: Pull pending jobs on Pusher reconnect
- `RES-04`: Crash recovery via `status='printing'` fence

**Out of scope (belongs elsewhere):**
- Heartbeat / observability dashboard (`OBS-01`) — v2 requirement, out of roadmap
- Print status reporting via ESC/POS `DLE EOT` — v2, serial only
- Auto-update — Phase 7
</domain>

<decisions>
## Implementation Decisions

### Schema migration v2 — D-01

- **D-01:** Add **migration v2** to `src/config_store.rs`. SQLite cannot `ALTER TABLE` to
  modify a CHECK constraint in place; the standard workaround is to recreate the table:

  ```sql
  -- v2: add 'printing' intermediate status for crash recovery (RES-04)
  CREATE TABLE printed_jobs_v2 (
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
  CREATE INDEX idx_printed_jobs_status ON printed_jobs(status);
  ```

  This is a single `M::up` added as the second element in the `MIGRATIONS` vec in
  `config_store.rs`. The existing `user_version` check in `rusqlite_migration` ensures
  this migration runs exactly once. Old data survives (all existing statuses are in the
  new CHECK — `pending`, `printed`; `failed` was already in v1).

### `status='printing'` fence in print_worker — D-02

- **D-02:** In `run_print_worker`, add a `status='printing'` UPDATE **before** calling
  `printer.print_raw()`. This is the crash recovery fence (RES-04): if the process dies
  mid-print, the row stays at `status='printing'`, which is how the retry task detects
  orphaned jobs at startup.

  Updated Phase 5 pipeline (additions bolded):

  | Step | Action |
  |------|--------|
  | 1 | Fetch bytes via `fetch_job_bytes()` |
  | **2** | **`UPDATE printed_jobs SET status='printing' WHERE job_id=?`** |
  | 3 | `printer.print_raw(&bytes)` |
  | 4a (success) | `UPDATE status='printed', printed_at=now` → `ack_job()` |
  | 4b (failure) | **`INSERT INTO retry_queue (job_id, job_type, escpos_bytes, attempt_count=1, next_retry_at=now+30s, last_error=msg, created_at=now)`** — leave status at 'printing' |

  On failure: leave `status='printing'` (intentional — the retry task finds these rows on
  startup for crash recovery, and knows to use `retry_queue.escpos_bytes` rather than
  re-fetching if the row is already in `retry_queue`).

  Note: the `status='printing'` UPDATE that currently fails silently (0 rows) should be
  treated the same as Phase 5's existing 0-row update handling — log and continue.

### Retry task architecture — D-03

- **D-03:** Create `src/retry_task.rs` with `pub async fn run_retry_task(...)`. This is a
  **fourth Tokio task** (alongside main event loop, Pusher task, print worker) that owns
  retry scheduling. Spawned in `main.rs` Runtime block alongside the Pusher and print
  worker tasks.

  Signature:
  ```rust
  pub async fn run_retry_task(
      db_path: PathBuf,
      agent_token: String,
      base_url: String,
      http: reqwest::Client,
      printer: Box<dyn Printer + Send>,
      send_health: impl Fn(HealthState) + Send + 'static,
  )
  ```

  The `Box<dyn Printer + Send>` is constructed in `main.rs` (reading `printer_name` /
  `printer_type` from ConfigStore, calling `printer_from_entry()`). The retry task and
  the print worker each hold their own `Box<dyn Printer>` — there is no shared printer
  resource (each `Printer` impl opens its own Win32 or serial handle on every
  `print_raw()` call, so two concurrent calls are safe and do not require a Mutex).

- **D-04:** The retry task opens its **own SQLite connection** (4th total, 3rd from async
  tasks) with `PRAGMA journal_mode=WAL`. Pattern: identical to `run_print_worker` and
  `run_pusher_loop`. Four WAL-mode connections to the same file is supported by SQLite's
  WAL reader/writer concurrency.

### Retry task startup — D-05

- **D-05:** At startup, before entering the poll loop, the retry task performs crash
  recovery (RES-04):

  ```sql
  -- Rows that crashed BEFORE bytes were saved to retry_queue
  SELECT job_id, job_type FROM printed_jobs
  WHERE status = 'printing'
    AND job_id NOT IN (SELECT job_id FROM retry_queue)
  ```

  For each such row: call `fetch_job_bytes()` to re-obtain the ESC/POS bytes, then
  `INSERT INTO retry_queue (job_id, job_type, escpos_bytes, attempt_count=1,
  next_retry_at=datetime('now'), last_error='crash recovery', created_at=datetime('now'))`.
  This schedules the job for immediate retry (next_retry_at=now). If `fetch_job_bytes()`
  fails, log and skip — the row stays at `status='printing'` and will be re-attempted on
  the next boot (idempotent startup check).

  Rows already in `retry_queue` at startup (crash between retries): these are found by the
  normal poll loop and retried per their stored `next_retry_at`.

### Retry task poll loop — D-06

- **D-06:** After startup, the retry task enters a poll loop with a **5-second interval**
  (`tokio::time::interval(Duration::from_secs(5))`):

  ```sql
  SELECT job_id, job_type, escpos_bytes, attempt_count
  FROM retry_queue
  WHERE next_retry_at <= datetime('now')
  ORDER BY next_retry_at ASC
  LIMIT 10
  ```

  For each row:
  1. Set `status='printing'` in `printed_jobs` (crash fence, same as D-02).
  2. Call `printer.print_raw(escpos_bytes)`.
  3. **On success:** `UPDATE printed_jobs SET status='printed', printed_at=datetime('now')` →
     `ack_job()` → `DELETE FROM retry_queue WHERE job_id=?`. Then call
     `send_health(HealthState::Connected)` to restore green tray (if a prior failure turned
     it red).
  4. **On failure, `attempt_count < 3`:** `UPDATE retry_queue SET attempt_count=attempt_count+1,
     next_retry_at=datetime('now', '+30 seconds'), last_error=<msg> WHERE job_id=?`.
     Leave `status='printing'` in printed_jobs.
  5. **On failure, `attempt_count >= 3`:** Exhaust the job:
     - `UPDATE printed_jobs SET status='failed', failed_at=datetime('now') WHERE job_id=?`
     - `DELETE FROM retry_queue WHERE job_id=?`
     - `send_health(HealthState::Problem)` — red tray
     - Show Windows toast (D-07)

### Windows toast notification (RES-02) — D-07

- **D-07:** Use `tauri-winrt-notification` (already in `Cargo.toml`). Show the toast after
  retry exhaustion (step 5 of D-06). Pattern from Phase 3 toast for failed print:

  ```rust
  let _ = Toast::new(Toast::POWERSHELL_APP_ID)
      .title("Brevly Print — Falha na impressão")
      .text1("Falha ao imprimir após 3 tentativas.")
      .text2("Verifique se a impressora está ligada e com papel.")
      .show();
  ```

  `Toast::POWERSHELL_APP_ID` (or the app-specific app_id established in Phase 3, if any)
  is used — same as Phase 3's notification pattern. If Phase 3 established a custom app_id
  in the registry, use that string here for consistency (planner/executor must check Phase 3
  implementation).

  The toast is **fire-and-forget** — `let _ = ...show()` (same as prior phases). Toast
  failures are not retried.

### Pending pull on reconnect (RES-03) — D-08

- **D-08:** After `subscription_succeeded` in `run_pusher_loop`, immediately call a new
  `noren_client::fetch_pending_jobs()` function. This happens on **every successful Pusher
  reconnect** (not only after detected outages). The dedup fence (`INSERT OR IGNORE`)
  handles already-printed jobs safely — the only cost of a spurious pull is one extra HTTP
  GET, which is acceptable.

  New function in `noren_client.rs`:
  ```rust
  // GET /api/agent/jobs/pending
  // Returns non-acked jobs sorted by createdAt ASC, max 100.
  // Expected response: JSON array [{"jobId": "...", "type": "..."}, ...]
  // (field names TBD — planner must confirm against Noren's actual contract)
  pub async fn fetch_pending_jobs(
      client: &reqwest::Client,
      base_url: &str,
      agent_token: &str,
  ) -> anyhow::Result<Vec<PendingJob>>

  pub struct PendingJob {
      pub job_id: String,
      pub job_type: String,
  }
  ```

  After fetching, the Pusher task iterates over `pending_jobs`, calls
  `insert_print_job(&pusher_conn, &job.job_id, &job.job_type)` (dedup fence), and if the
  row is new, sends a `PrintEvent` to the print worker via `tx.try_send()` (same path as
  live Pusher events, including the WR-04 backpressure handling).

  **Error handling:** if `fetch_pending_jobs()` fails (Noren not yet available, timeout),
  log the error and continue — do not reconnect the WebSocket. The pending pull is
  best-effort; the next reconnect will retry.

  **Dependency:** `GET /api/agent/jobs/pending` must be live in Noren before Phase 6
  completes (listed as a blocker in STATE.md). Local dev: use `BREVLY_FAKE_PUSHER_EVENT`
  for the Pusher path + a mock HTTP server or test stub for the pending pull.

### JSON field names for pending response — D-09

- **D-09:** The exact JSON field names for the pending response (`jobId` vs `job_id`,
  `type` vs `job_type`) are **TBD** pending the Noren contract. The planner must grep
  the Noren repo for the `/api/agent/jobs/pending` endpoint implementation to determine
  the actual field names used, then derive the `PendingJob` struct accordingly. Use a
  `#[serde(rename = "...")]` annotation if the Noren field names differ from the Rust
  snake_case convention.

### Health state label update — D-10

- **D-10:** Update `src/health_state.rs` `Problem` strings to reflect printer failure
  (not connection failure):

  | Field | Before (Phase 3) | After (Phase 6) |
  |-------|-----------------|-----------------|
  | `tooltip()` | `"Brevly Print — Problema de conexão"` | `"Brevly Print — Falha na impressora"` |
  | `status_label()` | `"Problema de conexão"` | `"Falha na impressora"` |

  The `Problem` state in Phase 6 is exclusively triggered by print failures, not
  connection issues (Pusher failures use `Reconnecting`). This wording better explains to
  the restaurant operator what's wrong.

### `run_print_worker` signature change — D-11

- **D-11:** Phase 6 does NOT add `send_health` to `run_print_worker`. The print worker
  only sets the `status='printing'` fence and writes to `retry_queue` on failure — the
  retry task owns the health state transitions (D-06). This keeps the print worker's
  responsibility unchanged: it handles the happy path and delegates failure follow-up.

### `retry_queue` INSERT on failure — D-12

- **D-12:** When `run_print_worker` encounters a print failure (step 4b of D-02):

  ```sql
  INSERT OR IGNORE INTO retry_queue
      (job_id, job_type, escpos_bytes, attempt_count, next_retry_at, last_error, created_at)
  VALUES
      (?1, ?2, ?3, 1, datetime('now', '+30 seconds'), ?4, datetime('now'))
  ```

  `INSERT OR IGNORE` prevents a double-insert if (somehow) the same job appears twice
  in the mpsc channel. `escpos_bytes` is the raw `Vec<u8>` fetched by `fetch_job_bytes()`
  — stored as BLOB (SQLite parameter binding `rusqlite::params![..., bytes.as_slice(), ...]`).

  The print worker does NOT update `attempt_count` in `retry_queue` — that is the retry
  task's exclusive responsibility (D-06, step 4).

### Claude's Discretion

- Exact SQL query parameter binding style for BLOB (planner choice — `&[u8]` vs `Vec<u8>`
  wrapper type).
- Whether `run_retry_task` uses a `tokio::time::interval` or `tokio::time::sleep` loop
  (both work; interval is more predictable for 5s polling).
- Error handling granularity in `fetch_pending_jobs()` — `anyhow::Result<Vec<PendingJob>>`
  is sufficient; typed errors for 401/403 (token expired) vs transport failure are optional.
- Test strategy: unit test for startup crash recovery query, unit test for retry exhaustion
  path (mock printer that always fails), integration test via BREVLY_FAKE_PUSHER_EVENT +
  mock HTTP.
- Whether the retry task `LIMIT 10` per tick should be tunable or hardcoded. Hardcode 10
  for now — the queue should never exceed a handful of rows in normal operation.
- Logging verbosity for retry attempts (log each attempt or only exhaustion and recovery).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase requirements & success criteria
- `.planning/ROADMAP.md` → **Phase 6: Resilience** — Goal + 4 Success Criteria (SC-1 through
  SC-4) that this phase is graded on.
- `.planning/REQUIREMENTS.md` → **RES-01 through RES-04** (all 4 Phase 6 requirements).

### Existing infrastructure Phase 6 extends
- `src/print_worker.rs` — **primary target**. D-02 adds `status='printing'` fence before
  `print_raw()` and D-12 adds `retry_queue` insert on failure. Read the full file before
  planning (esp. the status='printing' fence insertion point at lines ~136–145).
- `src/pusher/client.rs` — D-08 adds `fetch_pending_jobs()` call right after
  `subscription_succeeded` + `send_health(Connected)` (line ~289). Read the subscription_succeeded
  handling block before implementing.
- `src/config_store.rs` — D-01 adds migration v2. `MIGRATIONS` vec gets a second `M::up`.
  Read the existing v1 migration before writing v2 to match the style.
- `src/health_state.rs` — D-10 updates `Problem` tooltip and status_label strings.
- `src/noren_client.rs` — D-08/D-09 add `fetch_pending_jobs()`. Follow the `fetch_job_bytes()`
  / `ack_job()` pattern from Phase 5 for auth + error handling.
- `src/printer/mod.rs` — `printer_from_entry()` and `Printer` trait. The retry task
  constructs its own `Box<dyn Printer>` in `main.rs` (D-03) — same as Phase 5's approach.
  Read this to understand the trait bounds (`Send` is needed for async task).
- `src/main.rs` (Runtime block, lines ~395–465) — D-03 adds retry task spawn. Follow the
  existing print worker spawn pattern. The retry task needs `agent_token`, `base_url`,
  `db_path`, `http`, a `printer`, and a `proxy_for_retry` health closure.

### Phase 5 decisions carried forward
- `.planning/phases/05-job-pipeline/05-CONTEXT.md` →
  - **D-08** (Phase 5 status progression: pending→printed) — Phase 6 inserts `'printing'`
    between `pending` and `printed`, and adds `'failed'` as a terminal state.
  - **D-09** (ack failure policy: log + leave as 'printed') — unchanged; ack is retried
    naturally via RES-03 pending pull.
  - **WR-02** (skip ack on SQLite UPDATE failure) — preserved; Phase 6 does not alter the
    disabled-type branch of print_worker.

### Pitfalls & architecture constraints
- `.planning/STATE.md` → Accumulated Context, Critical Pitfalls:
  - **C3**: dedup fence (`INSERT OR IGNORE`) — Phase 6 relies on this for RES-03 pending pull.
    Pending jobs that were already printed are safely no-op'd by the dedup fence.
  - **C4**: ack after status='done' — preserved; Phase 6 adds `status='printing'` BEFORE
    the print call (D-02), so C4 ordering is: `'printing'` → `print_raw()` → `'printed'` → ack.
  - **C5**: Pusher zombie detection — unchanged; Phase 6 pending pull runs in the existing
    reconnect flow, after `subscription_succeeded`, before entering the inner event loop.
  - Cross-platform build: `retry_task.rs` must compile on Linux. `printer_from_entry()`'s
    stub impl returns `Ok(())` on Linux. `fetch_pending_jobs()` is pure async HTTP — no
    platform gate. Toast notification (`tauri-winrt-notification`) is `#[cfg(windows)]`.

### External dependency (Noren backend)
- `GET /api/agent/jobs/pending` must be live in Noren before Phase 6 completes (in STATE.md
  blockers). Returns non-acked jobs (`createdAt ASC`, max 100). Field name convention for
  `jobId`/`type` vs `job_id`/`job_type` must be confirmed against the Noren implementation
  before coding `PendingJob` struct (D-09).

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- **`src/pusher/client.rs` `insert_print_job()`**: the dedup fence — Phase 6 reuses this
  function unchanged for the pending pull (D-08). After `fetch_pending_jobs()`, iterate and
  call `insert_print_job(&pusher_conn, &job.job_id, &job.job_type)` exactly as for live events.
- **`src/pusher/client.rs` `try_send` pattern (WR-04)**: the backpressure-safe mpsc send
  for pending pull results — use the same `tx.try_send(event)` + spawn-on-Full pattern.
- **`src/noren_client.rs` `fetch_job_bytes()` / `ack_job()`**: HTTP auth pattern (bearer
  token, anyhow::Result, non-200 bail). `fetch_pending_jobs()` follows this exactly.
- **`src/printer/mod.rs` `printer_from_entry()`**: used in both `main.rs` (for the retry
  task's `Box<dyn Printer>`) and Phase 5's print worker startup — the function signature
  and trait bound (`Box<dyn Printer>`) are already established.
- **`src/pusher/backoff.rs` `backoff_delay()`**: available for retry-task backoff if wanted,
  but the retry task uses a fixed 30s interval per spec — not exponential backoff.

### Established Patterns
- **4th WAL connection**: the retry task adds a 4th concurrent SQLite connection (main,
  Pusher, print worker, retry). All use `PRAGMA journal_mode=WAL`. WAL supports multiple
  concurrent readers and one writer; the retry task is the only writer to `retry_queue`, so
  there is no write contention on that table. `printed_jobs` may have concurrent writes from
  the print worker and retry task — the WAL locking protocol handles this safely.
- **`tokio::spawn` in Runtime block**: follow the exact pattern used for pusher and print
  worker. Pass `db_path.clone()`, `http.clone()`, `agent_token.clone()`, `base_url.clone()`.
- **`impl Fn(HealthState) + Send + 'static` health closure**: same closure type as
  `run_pusher_loop`. In `main.rs`: `let proxy_for_retry = event_loop.create_proxy();`
  then `let retry_send_health = move |state: HealthState| { let _ = proxy_for_retry.send_event(UserEvent::HealthChanged(state)); };`.
- **`#[cfg(windows)]` toast**: wrap the `tauri-winrt-notification` call in `#[cfg(windows)]`
  (the toast crate is already a `[target.'cfg(windows)'.dependencies]` entry). On Linux, the
  code path for retry exhaustion should log to stderr instead.
- **`eprintln!("[brevly-print] ...")` logging**: Phase 6 uses the same prefix as all prior
  phases. Suggest prefixes: `"[brevly-print] Retry task: ..."`, `"[brevly-print] Pusher: pending pull: ..."`.

### Integration Points
- `main.rs` Runtime block:
  - Read `printer_name` + `printer_type` from ConfigStore a SECOND time (for retry task printer).
  - Create `proxy_for_retry` and `retry_send_health` closure.
  - `rt_handle.spawn(async move { run_retry_task(db_path.clone(), ...).await; });`
  - Order: spawn Pusher, spawn print worker, spawn retry task (any order is fine — all are independent).
- `lib.rs`: add `pub mod retry_task;` alongside existing module declarations.
- `src/config_store.rs` `MIGRATIONS` vec: add second `M::up(...)` for migration v2 (D-01).
- `src/print_worker.rs`: insert `status='printing'` UPDATE between `fetch_job_bytes` and
  `print_raw`, and insert `retry_queue` INSERT after `print_raw` failure (D-02, D-12).
- `src/pusher/client.rs`: after the `send_health(Connected)` call at line ~290, add
  `fetch_pending_jobs()` call + iterate + try_send loop (D-08).
- `src/health_state.rs`: update 2 string literals for `Problem` variant (D-10).

</code_context>

<specifics>
## Specific Details

- **SQLite v2 migration**: must use the table-recreation pattern (D-01) — SQLite does not
  support `ALTER TABLE ... MODIFY COLUMN` or constraint changes in place.
- **`retry_queue.escpos_bytes` BLOB binding**: use `rusqlite::params![job_id, job_type, bytes.as_slice(), ...]`
  where `bytes: Vec<u8>`. Reading back: `row.get::<_, Vec<u8>>(col_idx)`.
- **4th SQLite connection note**: SQLite's WAL mode supports many concurrent readers and
  serializes writers. With 4 connections all in WAL mode, the only risk is write-write
  contention on `printed_jobs` between `run_print_worker` and `run_retry_task`. SQLite
  handles this via its WAL lock — one writer proceeds, the other retries briefly. No Mutex
  needed at the Rust level.
- **Toast app_id**: check `src/main.rs` or `src/tray_runtime.rs` for the Phase 3 toast
  implementation (if any) to reuse the same app_id. If no prior toast was implemented,
  use `Toast::POWERSHELL_APP_ID` as the fallback (works without registration).
- **Pending pull field names**: the Noren API might use `camelCase` (`jobId`, `type`) or
  `snake_case` (`job_id`, `job_type`). Use `#[serde(rename_all = "camelCase")]` on
  `PendingJob` if the Noren contract uses camelCase (most likely, as it's a JS/TS backend).
  Planner must check.
- **Retry task printer trait bound**: `Box<dyn Printer + Send>` — verify `Printer` trait
  in `src/printer/mod.rs` has `+ Send` in its supertrait bounds. If not, Phase 6 must add
  `Send` to the trait definition (breaking change to all impls — all must also be `Send`).
- **Blocker**: Phase 6 completion is gated on Noren shipping:
  - `GET /api/agent/jobs/pending` (non-acked jobs, `createdAt ASC`, max 100)
  The retry path (RES-01/02/04) can be fully tested without Noren using a mock printer
  that fails N times. The pending pull path (RES-03) needs either a mock HTTP server or
  a live Noren endpoint.
</specifics>

<deferred>
## Deferred Ideas

- **Heartbeat to Noren** (`OBS-01`) — would let the Noren dashboard show "impressora
  online/offline" and detect antivirus quarantine of the EXE. v2 requirement; not in
  Phase 6 scope.
- **Paper-level sensing via `DLE EOT`** — ESC/POS status request over serial; useful for
  "Impressora sem papel" vs "Impressora offline" toast copy. Requires serial connection
  and a Noren status API change. v2, serial only.
- **Typed retry errors** — differentiating "printer offline" vs "paper out" vs "USB
  disconnect" would enable more specific toast messages (D-07 uses generic copy). Deferred
  until hardware-specific error codes are profiled on the target printer.
- **Retry count configurability** — ROADMAP specifies 3× / 30s; making these configurable
  is a v2 nicety not needed for MVP.

None of the above affect Phase 6 scope.

</deferred>

---

*Phase: 06-resilience*
*Context gathered: 2026-07-16*
