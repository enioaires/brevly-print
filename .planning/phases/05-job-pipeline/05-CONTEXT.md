# Phase 5: Job Pipeline - Context

**Gathered:** 2026-07-16
**Status:** Ready for planning
**Note:** Owner delegated all decisions ("pode decidir tudo e já criar o contexto, não entendo
muito sobre a parte técnica") — same posture as Phases 1, 3, and 4. Every decision below is
**locked** (Claude's discretion, exercised). Downstream agents should treat these as decided,
not open.

<domain>
## Phase Boundary

Complete the end-to-end print path. The Phase 4 Pusher task already deposits `PrintEvent`s
into an `mpsc` channel and writes the C3 dedup fence row (`status='pending'`) to SQLite.
Phase 5 picks those events up, fetches ESC/POS bytes from Noren, sends them to the thermal
printer, and acknowledges delivery — closing the loop.

Concretely:

1. **Print worker task** — A tokio task (parallel to the Pusher task) that owns the
   `mpsc::Receiver<PrintEvent>`. For each event it drives the full pipeline below.
2. **enabled_types filter (PRT-09)** — If the job's `job_type` is not in `enabled_types`
   (read from ConfigStore): mark `status='printed'`, send ack, no print, no error.
3. **Fetch bytes (PRT-01)** — `GET /api/agent/jobs/{jobId}/bytes` → base64-decode → `Vec<u8>`.
4. **Print (PRT-02/03/04/05/06)** — Call `printer.print_raw(&bytes)` via the existing
   `Box<dyn Printer>` (spooler or serial path, already implemented in Phase 2).
5. **Mark done (PRT-07/08)** — `UPDATE printed_jobs SET status='printed', printed_at=NOW
   WHERE job_id=?`. Write BEFORE ack (C4 constraint).
6. **Ack (PRT-08)** — `POST /api/agent/jobs/{jobId}/ack`. Fire once; ack failure is logged
   and left for Phase 6's pending pull to recover.

All 9 Phase 5 requirements (PRT-01..PRT-09) are covered in the happy path.
Failures (fetch error, print error) are logged and left as `status='pending'` — Phase 6
(retry, crash recovery) owns failure-path behavior.

**Out of scope (belongs to other phases):**
- Printer-failure retry (3× / 30s), Windows toast notifications — **Phase 6 (RES-01/RES-02)**.
- `retry_queue` writes — **Phase 6** owns this table entirely.
- Internet-outage pending pull (`GET /api/agent/jobs/pending`) — **Phase 6 (RES-03)**.
- Crash recovery / `status='printing'` fence — **Phase 6 (RES-04)**.
- `'printing'` intermediate status in schema — **Phase 6** adds migration v2.
- Auto-update — **Phase 7**.
</domain>

<decisions>
## Implementation Decisions

### Print worker module and spawn point — D-01
- **D-01:** **Create `src/print_worker.rs`** with `pub async fn run_print_worker(rx, agent_token,
  base_url, db_path, http)` — analogous to `src/pusher/client.rs` / `run_pusher_loop()`. Spawned
  in `main.rs` Runtime block immediately after the Pusher task spawn. `print_rx` goes directly to
  the worker task; it is NOT stored in `App` any longer.
- **D-02:** **Remove `App._print_rx: Option<Receiver<PrintEvent>>`** from the `App` struct. That
  field was a Phase 4 placeholder ("Phase 5 will take it"). Phase 5 consumes the receiver at
  spawn time; `App` no longer needs to hold it. `lib.rs` gets `pub mod print_worker`.

### Print worker startup — D-03
- **D-03:** The print worker opens its own `rusqlite::Connection` with `PRAGMA journal_mode=WAL`
  (same pattern as the Pusher task — two concurrent connections, both WAL). At startup, before
  entering the event loop, it reads from ConfigStore:
  - `enabled_types` → `serde_json::from_str::<Vec<String>>()` (stored by Phase 2 as JSON)
  - `printer_name` + `printer_type` → construct `PrinterId::Spooler(name)` or `PrinterId::Serial(port)`
    → `printer_from_entry(&id)` → hold `Box<dyn Printer>` for the task lifetime.
  - If any read fails or `printer_name` is missing: log fatal error and return (unreachable in
    production — Phase 2 guarantees these keys).
  - If `enabled_types` is missing or the JSON is empty: default to allowing all types (fail-safe).

### Noren HTTP additions — D-04
- **D-04:** **`noren_client.rs` gets two new async functions** (same `anyhow::Result` pattern as
  `pusher_auth()`):

  ```rust
  // GET /api/agent/jobs/{jobId}/bytes
  // Response JSON: {"bytes": "<base64>"}
  // Returns the base64-decoded bytes ready for print_raw().
  pub async fn fetch_job_bytes(
      client: &reqwest::Client, base_url: &str, agent_token: &str, job_id: &str,
  ) -> anyhow::Result<Vec<u8>>

  // POST /api/agent/jobs/{jobId}/ack  (idempotent)
  // 200 → Ok(()); 409 → Ok(()) (already acked); others → Err.
  pub async fn ack_job(
      client: &reqwest::Client, base_url: &str, agent_token: &str, job_id: &str,
  ) -> anyhow::Result<()>
  ```

  `agent_token` is passed via `.bearer_auth()` — never logged (T-02-02). 409 on ack is treated
  as `Ok(())` — idempotent by design (C4 pitfall: ack 409 after a crash-and-retry is normal).

### New dependency — D-05
- **D-05:** Add `base64 = "0.22"` to `[dependencies]` in `Cargo.toml` (portable — no
  `[target.cfg(windows)]` gate needed). Used by `fetch_job_bytes()` to decode the base64-encoded
  ESC/POS bytes returned by Noren. The `base64::engine::general_purpose::STANDARD` engine decodes
  the `bytes` field from the JSON response.

### Job type string values — D-06
- **D-06:** The `job_type` field in `PrintEvent` will be one of the following strings (matching
  what Noren emits in the Pusher event payload):

  | String | Comanda | Requirement |
  |--------|---------|-------------|
  | `"order"` | Comanda de pedido (pedido novo confirmado) | PRT-02 |
  | `"dispatch"` | Comanda do entregador (despacho, com QR) | PRT-03 |
  | `"closing"` | Cupom de fechamento de caixa | PRT-04 |
  | `"cancel"` | Comanda de cancelamento (bonus, PROJECT.md) | — |

  **MUST VERIFY** these exact strings against Noren's event emission code before Phase 5
  completes (the strings must match what `enabled_types` contains). The print worker doesn't
  route behavior by type — all types use the same fetch→print path. Type is only used for
  the `enabled_types` filter (D-07).

### enabled_types filter (PRT-09) — D-07
- **D-07:** Per event in the print worker:
  1. If `event.job_type` is in `enabled_types` (or `enabled_types` is empty/missing): proceed
     to fetch + print.
  2. If `event.job_type` is NOT in `enabled_types`: mark `status='printed'` + send ack + skip
     print. No error raised (PRT-09 spec). The row is marked 'printed' so Phase 6 doesn't
     re-queue it as a missed job.

### SQLite status progression — D-08
- **D-08:** Phase 5 status transitions (schema v1 — no migration needed):

  | From | To | When |
  |------|-----|------|
  | `pending` | `printed` | Successful `print_raw()` call |
  | `pending` | `printed` | Disabled job type (D-07) |
  | `pending` | *(unchanged)* | Fetch or print failure — logged, left for Phase 6 |

  `printed_at = datetime('now')` written alongside `status='printed'`.
  No `'printing'` intermediate state (deferred to Phase 6 for crash recovery).
  No `'failed'` writes in Phase 5 (that marker is Phase 6's exhaustion signal after 3× retry).
  Phase 5 does NOT write to `retry_queue` — Phase 6 owns that table.

### Ack failure policy — D-09
- **D-09:** Order: `UPDATE status='printed'` **then** `ack_job()` (C4 constraint — status first,
  ack second). If the ack POST fails: log the error, leave status as `'printed'`. Phase 6's
  `GET /api/agent/jobs/pending` pulls all non-acked jobs and re-delivers them; the C3 dedup
  fence (`INSERT OR IGNORE`) will no-op the re-delivery since the job is already in
  `printed_jobs` — the ack is simply retried. **No inline ack retry in Phase 5.**

### Claude's Discretion (delegated — planner/executor finalize)
- Exact function signature and internal structure of `run_print_worker()` — mirrors
  `run_pusher_loop()` but planner decides on logging verbosity and error formatting.
- Whether to use `while let Some(event) = rx.recv().await` or `loop { ... }` in the worker.
- Error handling granularity in `fetch_job_bytes()` — `anyhow::Result` is sufficient;
  typed error variants for 404 (job not found) vs transport failure are optional.
- SQL update helper — inline `conn.execute()` or a small `update_job_status()` helper function.
- Test coverage — unit tests for `fetch_job_bytes()` deser + ack 409 handling; integration
  test via `BREVLY_FAKE_PUSHER_EVENT` env var (already in Phase 4) plus a fake HTTP server
  or mock, planner's choice.
</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase requirements & success criteria
- `.planning/ROADMAP.md` → **Phase 5: Job Pipeline** — Goal + 6 Success Criteria that this
  phase is graded on (SC-1: <1s; SC-2: all 3 ticket types; SC-3: dedup; SC-4: ack after done;
  SC-5: disabled types; SC-6: USB + serial).
- `.planning/REQUIREMENTS.md` → **PRT-01 through PRT-09** (all 9 Phase 5 requirements).

### Existing infrastructure Phase 5 extends
- `src/pusher/client.rs` — `run_pusher_loop()` — **template for `run_print_worker()`**. Mirrors:
  WAL-mode connection open, mpsc receiver loop, anyhow error handling, eprintln logging pattern.
- `src/printer/mod.rs` — `Printer` trait, `printer_from_entry()`, `PrinterId` enum,
  `PrinterError`. Phase 5 calls `printer.print_raw(&bytes)` — the entire spooler/serial
  abstraction is already built.
- `src/printer/spooler.rs` — `WindowsSpoolerPrinter` — WritePrinter RAW (C1: pDatatype="RAW"
  critical; validated in Phase 2 test-print button). Phase 5 relies on this without changes.
- `src/printer/serial.rs` — `SerialPrinter` — COM port `serialport::write()` (PRT-05, PRT-06).
- `src/noren_client.rs` — `noren_base_url()`, `pusher_auth()` — pattern for new HTTP functions.
  Phase 5 adds `fetch_job_bytes()` + `ack_job()` here.
- `src/config_store.rs` — Schema v1 (`config`, `printed_jobs`, `retry_queue`). `get()` API
  used by print worker to read `enabled_types`, `printer_name`, `printer_type` at startup.
- `src/main.rs` — Runtime block (lines ~389–449): where Phase 5 removes `App._print_rx` and
  adds the print worker spawn alongside the Pusher task spawn.

### Phase 4 decisions carried forward
- `.planning/phases/04-pusher-event-stream/04-CONTEXT.md` →
  - **D-03** (hybrid handoff: `INSERT OR IGNORE` fence + mpsc send) — Phase 5 receives events
    via mpsc; the SQLite row is already written by Phase 4.
  - **D-04** (`BREVLY_FAKE_PUSHER_EVENT` dev shim) — still active; Phase 5's print path is
    exercisable end-to-end without Noren's live endpoints.
  - Phase 5 must not touch `App.conn` from the async task (C2 / Pitfall 4 applies generally:
    non-event-loop threads must not access tray or main-thread state directly).

### Pitfalls & architecture constraints
- `.planning/STATE.md` → Accumulated Context, especially:
  - **C1**: RAW datatype omitted in WritePrinter → ESC/POS becomes garbage. Already validated
    in Phase 2 test-print, but Phase 5 must not alter the printer call path.
  - **C3**: dedup in-memory lost on crash → SQLite `INSERT OR IGNORE` is the fence. Phase 4
    owns the insert; Phase 5 must update the row to `'printed'` BEFORE acking (C4).
  - **C4**: ack before status='done' (='printed') → job silently lost. Order: UPDATE → ack.
  - Cross-platform build constraint: `print_worker.rs` must compile on Linux. `printer_from_entry()`
    and `Printer::print_raw()` compile on Linux (stub impl); `fetch_job_bytes()` and `ack_job()`
    are pure async HTTP — no platform gate needed.
</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- **`src/pusher/client.rs`**: `run_pusher_loop()` — exact structural template for
  `run_print_worker()`. Copy the WAL-mode connection open, the mpsc `while let Some(event)` loop,
  and the `eprintln!("[brevly-print] ...")` logging style.
- **`src/printer/mod.rs`**: `printer_from_entry(&PrinterId) -> Box<dyn Printer>` — Phase 5
  calls this at startup. `PrinterId::Spooler(name)` for `printer_type == "spooler"`;
  `PrinterId::Serial(port)` for `printer_type == "serial"`.
- **`src/noren_client.rs`**: `pusher_auth()` — the HTTP pattern to replicate for
  `fetch_job_bytes()` and `ack_job()`. Uses `.bearer_auth(agent_token)`, `.send().await?`,
  `.json::<T>().await?`, `anyhow::bail!` for non-200.
- **`src/config_store.rs`**: `config_store::get(conn, "enabled_types")` → `Option<String>`,
  then `serde_json::from_str::<Vec<String>>(&json_str)`. Same pattern for `printer_name` /
  `printer_type`.

### Established Patterns
- **WAL-mode second connection**: `rusqlite::Connection::open(&db_path)` +
  `conn.pragma_update(None, "journal_mode", "WAL")` — the Pusher task does this; Phase 5 mirrors it.
- **anyhow::Result for internal errors**: consistent across `noren_client.rs`, `pusher/client.rs`.
  New functions follow this convention; no new typed error enums needed for HTTP calls.
- **`#[cfg(debug_assertions)]` dev shim**: the `BREVLY_FAKE_PUSHER_EVENT` shim is already
  active in Phase 4. Phase 5 can verify the full pipeline via this shim + a real (or mocked)
  `fetch_job_bytes()` endpoint, without needing live Pusher.
- **Cross-platform compilation**: `print_worker.rs` itself has no Windows-specific imports.
  The `Printer` trait's `stub.rs` impl returns `Ok(())` on Linux, so the full async pipeline
  compiles and runs on Linux CI.

### Integration Points
- `main.rs` Runtime block → remove `_print_rx: Option<...>` from `App`, spawn
  `run_print_worker(print_rx, agent_token.clone(), base_url.clone(), db_path.clone(), http.clone())`.
  Both `agent_token` and `base_url` are already read in the Runtime block for the Pusher task.
- `lib.rs` → `pub mod print_worker` (alongside `pub mod pusher`, `pub mod printer`, etc.)
- `noren_client.rs` → new `fetch_job_bytes()` + `ack_job()` functions at the bottom of the file.
- `Cargo.toml` → `base64 = "0.22"` in `[dependencies]` (portable).
- `printed_jobs` table → Phase 5 executes: `UPDATE printed_jobs SET status='printed',
  printed_at=datetime('now') WHERE job_id=?` after a successful print (and for disabled-type
  jobs too — D-07).
</code_context>

<specifics>
## Specific Details

- **`fetch_job_bytes` response shape**: `GET /api/agent/jobs/{jobId}/bytes` returns
  `{"bytes": "<base64-string>"}`. Phase 5 deserializes with a local `struct BytesResponse { bytes: String }`,
  then decodes via `base64::engine::general_purpose::STANDARD.decode(&body.bytes)`.
- **`ack_job` endpoint**: `POST /api/agent/jobs/{jobId}/ack` with `Authorization: Bearer {agentToken}`
  and an empty body (or `{}`). Status mapping: 200 → `Ok(())`; 409 → `Ok(())` (idempotent,
  C4 pitfall: a post-crash ack is normal); others → `anyhow::bail!`.
- **`enabled_types` key**: stored in ConfigStore as JSON, e.g. `["order","dispatch","closing"]`.
  Saved by Phase 2 activation window at line 984 of `src/activation_window.rs`.
- **`base64` version**: `0.22` — current stable. API: `use base64::Engine as _;` +
  `base64::engine::general_purpose::STANDARD.decode(s)`.
- **Job type strings to verify**: `"order"`, `"dispatch"`, `"closing"`, `"cancel"` are the
  expected values. **Before Phase 5 ships: grep Noren's event emission code for the exact
  strings used in the Pusher trigger call to confirm.**
- **Blocker**: Phase 5 completion is gated on Noren shipping:
  - `GET /api/agent/jobs/{jobId}/bytes` (ESC/POS bytes, base64-encoded)
  - `POST /api/agent/jobs/{jobId}/ack` (idempotent, 409 on repeat)
  - Server-side ESC/POS rendering complete (migrated from `ticket.ts`, ISO-8859-1 preserved)
  Phase 4's `BREVLY_FAKE_PUSHER_EVENT` shim + a mock HTTP server lets the full pipeline
  be verified locally before Noren's endpoints ship.
</specifics>

<deferred>
## Deferred Ideas

- **`status='printing'` intermediate fence** — RES-04 (Phase 6) mentions this for crash
  recovery. Phase 5 doesn't implement it; the `'printing'` status and the schema migration v2
  that adds it to the CHECK constraint belong to Phase 6.
- **Inline ack retry** — Phase 5 fires ack once. If Noren later wants guaranteed-ack delivery
  with inline retry in Phase 5, that's a refinement; for now Phase 6's pending pull covers it.
- **Fetch failure typed errors** — differentiating 404 (job not found, server deleted it) vs
  transport error could let Phase 5 treat 404 as "mark 'printed', no retry" vs "leave 'pending'".
  Deferred to Phase 6 when retry semantics are defined.
- **Toast notification on print failure** — Phase 6 (RES-02). Phase 5 only logs to stderr.

None of the above expanded Phase 5 scope — all belong to Phase 6 or later.
</deferred>

---

*Phase: 05-job-pipeline*
*Context gathered: 2026-07-16*
