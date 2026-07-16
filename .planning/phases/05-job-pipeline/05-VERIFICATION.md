---
phase: 05-job-pipeline
verified: 2026-07-16T00:00:00Z
status: human_needed
score: 9/9 must-haves verified
overrides_applied: 0
human_verification:
  - test: "Trigger a real print event in Noren and measure time from Pusher event arrival to physical comanda printout on a thermal printer"
    expected: "Comanda sai na impressora em menos de 1 segundo (PRT-06 / SC-1)"
    why_human: "< 1 second latency requires physical hardware (Windows + thermal printer + live Pusher connection). Cannot be proven via grep or cargo test."
  - test: "Trigger pedido, despacho, and fechamento events in Noren; verify each prints correctly with correct content and QR code scanning as dispatch token"
    expected: "PRT-02/03/04: three ticket types each produce correct output; QR on despacho ticket scans as the dispatch token"
    why_human: "Physical output correctness requires visual inspection of printed content and a QR scanner. Cannot be verified in CI."
  - test: "Trigger a print event and verify the agent prints via USB spooler (WritePrinter RAW datatype) AND via a printer connected on a COM port"
    expected: "Both paths (Spooler and Serial) successfully print ESC/POS bytes with no datatype conversion (PRT-05 / SC-6)"
    why_human: "Requires two physical printer configurations on Windows. Linux CI runs StubPrinter (no-op). Cannot be verified without hardware."
  - test: "Confirm the job_type strings emitted by Noren (e.g. 'order', 'dispatch', 'closing') match exactly the values stored in enabled_types config (D-06 grep)"
    expected: "The strings Noren puts in {type} field match what enabled_types filter uses — no silent mis-routing"
    why_human: "Requires access to the Noren repo's event-emission source code to grep for the actual string constants. Cannot verify without cross-repo access."
---

# Phase 5: Job Pipeline Verification Report

**Phase Goal:** Every print event results in the correct ESC/POS bytes being written to the thermal printer within 1 second, with delivery confirmed back to Noren and duplicate prints prevented
**Verified:** 2026-07-16
**Status:** human_needed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `fetch_job_bytes` returns base64-decoded ESC/POS bytes from a 200 `{"bytes":"<b64>"}` response | VERIFIED | `src/noren_client.rs` lines 230-265: GET → BytesResponse → `STANDARD.decode(&body.bytes)`; test `test_fetch_job_bytes_200_decodes_base64` passes |
| 2 | `fetch_job_bytes` returns Err on a non-200 status | VERIFIED | `src/noren_client.rs` line 263: `status => anyhow::bail!(...)`; test `test_fetch_job_bytes_non_200_returns_err` passes |
| 3 | `ack_job` returns Ok(()) on both 200 and 409 (idempotent) | VERIFIED | `src/noren_client.rs` line 292: `200 | 409 => Ok(())`; test `test_ack_job_409_returns_ok` passes |
| 4 | `ack_job` returns Err on unexpected status (e.g. 500) | VERIFIED | `src/noren_client.rs` line 293: `status => anyhow::bail!(...)`; test `test_ack_job_500_returns_err` passes |
| 5 | `agent_token` is passed via `bearer_auth` and never appears in any log or error string | VERIFIED | `src/noren_client.rs` lines 248, 286: `.bearer_auth(agent_token)` with comment confirming T-02-02; `src/print_worker.rs` passes token only as function arg — no eprintln!/format! containing token |
| 6 | A PrintEvent whose job_type is in enabled_types is fetched and printed via `printer.print_raw` | VERIFIED | `src/print_worker.rs` lines 90-108 (filter), 113-133 (fetch + print_raw); `enabled_types_filter` test passes |
| 7 | A PrintEvent whose job_type is NOT in enabled_types is marked status='printed' + acked without printing, no error raised | VERIFIED | `src/print_worker.rs` lines 90-109: disabled-type branch executes UPDATE + ack_job then `continue` — no print_raw call; `enabled_types_filter` test verifies predicate |
| 8 | When enabled_types is missing or empty, all job types are printed (fail-safe allow-all) | VERIFIED | `src/print_worker.rs` line 58-61: `.unwrap_or_default()` yields empty Vec; line 90: `!enabled_types.is_empty()` guard ensures empty list skips the disabled branch; test asserts `is_allowed(&[], "closing") == true` |
| 9 | The SQLite UPDATE to status='printed' happens BEFORE ack_job() on every path (C4) | VERIFIED | `src/print_worker.rs` lines 92-102 (disabled-type path: UPDATE then ack) and lines 137-150 (success path: UPDATE then ack); `update_precedes_ack_in_source` test statically asserts first UPDATE index (93) < last ack_job index (150) |

**Score:** 9/9 truths verified

**Note on ROADMAP SC-4 wording:** ROADMAP says `status = 'done'`; the plan, context, schema, and implementation consistently use `status = 'printed'`. This is a stale ROADMAP wording — the semantic intent (write status before ack) is fully implemented and tested. Not a functional gap.

**Note on Roadmap SC-3 (PRT-07 dedup):** The `INSERT OR IGNORE INTO printed_jobs` dedup fence lives in `src/pusher/client.rs` (Phase 4 deliverable). Phase 5 consumes events that are already deduplicated at the Pusher layer. The PLAN frontmatter correctly assigns PRT-07 to Phase 4 plan 04-02. The dedup is in place and tested (Phase 4 tests cover it).

**Note on Roadmap SC-1/2/6 (PRT-06 latency, physical correctness, dual-path printing):** These require hardware verification — see Human Verification section.

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `Cargo.toml` | `base64 = "0.22"` direct dep | VERIFIED | Line 46: `base64 = "0.22"` under `[dependencies]` (portable, not Windows-gated) |
| `src/noren_client.rs` | `fetch_job_bytes()` and `ack_job()` async HTTP functions | VERIFIED | Lines 230-295: both functions present, substantive, used by tests and print_worker |
| `src/print_worker.rs` | `run_print_worker()` async task — full fetch→print→update→ack pipeline | VERIFIED | Lines 28-159: complete 159-line implementation; startup, event loop, all branches |
| `src/main.rs` | print worker spawned in Runtime block; `_print_rx` removed from App | VERIFIED | Lines 452-454: `rt_handle.spawn(async move { run_print_worker(...).await; })` present; `_print_rx` grep returns 0 matches |
| `src/lib.rs` | `pub mod print_worker;` module declaration | VERIFIED | Line 17: `pub mod print_worker;` |
| `src/printer/mod.rs` | `Printer` trait with `Send` bound | VERIFIED | Line 51: `pub trait Printer: Send` — required for `Box<dyn Printer>` across `.await` in `tokio::spawn` |
| `tests/print_worker_test.rs` | All 6 tests pass (4 Wave-0 + 2 Plan 02) | VERIFIED | `cargo test -q --test print_worker_test` → 6 passed; 0 failed |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/noren_client.rs::fetch_job_bytes` | `base64::engine::general_purpose::STANDARD` | `.decode(&body.bytes)` | VERIFIED | Line 259: `STANDARD.decode(&body.bytes)` — Engine API, not deprecated 0.21 free functions |
| `src/noren_client.rs::ack_job` | `reqwest bearer_auth` | `.bearer_auth(agent_token)` | VERIFIED | Line 286: `.bearer_auth(agent_token)` |
| `src/main.rs Runtime block` | `print_worker::run_print_worker` | `rt_handle.spawn(run_print_worker(print_rx, ...))` | VERIFIED | Lines 26 (import) + 452-454 (spawn) — 2 occurrences as required |
| `src/print_worker.rs` | `printer.print_raw` | `Box<dyn Printer>::print_raw(&bytes)` | VERIFIED | Line 127: `printer.print_raw(&bytes)` |
| `src/print_worker.rs UPDATE` | `printed_jobs status='printed'` | `conn.execute UPDATE before ack_job` | VERIFIED | Lines 92-102 (disabled path) and 137-150 (success path): UPDATE precedes ack on both paths |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|--------------|--------|-------------------|--------|
| `src/print_worker.rs` | `bytes` (ESC/POS) | `fetch_job_bytes()` → `reqwest GET` → `STANDARD.decode()` | Yes — network fetch + base64 decode | FLOWING |
| `src/print_worker.rs` | `enabled_types` | `config_store::get(&conn, "enabled_types")` → SQLite | Yes — reads real SQLite row | FLOWING |
| `src/print_worker.rs` | `printer` | `printer_from_entry(&printer_id)` from `config_store` printer_name/type | Yes — reads real config | FLOWING |
| `src/main.rs` | `print_rx` | `tokio::sync::mpsc::channel` → `run_pusher_loop` sends `PrintEvent` | Yes — from Pusher WebSocket events | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| All 6 print_worker tests pass | `cargo test -q --test print_worker_test` | 6 passed; 0 failed | PASS |
| Full crate builds without errors | `cargo build -q` | 0 crates compiled (cached clean) | PASS |
| Full test suite passes | `cargo test -q` | 40 passed, 1 ignored | PASS |
| UPDATE precedes ack_job in source | static test `update_precedes_ack_in_source` | index 93 (UPDATE) < index 150 (ack_job) | PASS |
| `run_print_worker` import + spawn count | `grep -c "run_print_worker" src/main.rs` | 2 | PASS |
| `_print_rx` fully removed | `grep -c "_print_rx" src/main.rs` | 0 | PASS |

### Probe Execution

Step 7c: SKIPPED — no `scripts/*/tests/probe-*.sh` files found; no probe paths declared in PLAN files.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| PRT-01 | 05-01-PLAN.md | Fetch ESC/POS bytes via HTTP authenticated | SATISFIED | `fetch_job_bytes()` in `noren_client.rs`; 2 contract tests pass |
| PRT-02 | 05-02-PLAN.md | Print comanda de pedido (order ticket) | NEEDS HUMAN | `print_raw()` call wired; physical output requires hardware |
| PRT-03 | 05-02-PLAN.md | Print comanda do entregador (QR de despacho) | NEEDS HUMAN | `print_raw()` call wired; QR correctness requires physical print + scanner |
| PRT-04 | 05-02-PLAN.md | Print cupom de fechamento | NEEDS HUMAN | `print_raw()` call wired; physical output requires hardware |
| PRT-05 | 05-02-PLAN.md | Print via WritePrinter RAW and serial COM | NEEDS HUMAN | `Spooler`/`Serial` branches in `printer_from_entry` wired; Linux CI uses StubPrinter |
| PRT-06 | 05-02-PLAN.md | Comanda in < 1 second after event | NEEDS HUMAN | Pipeline is synchronous (no queuing delay added); latency requires Windows hardware measurement |
| PRT-07 | 04-02-PLAN.md | Dedup via SQLite `INSERT OR IGNORE` | SATISFIED | Implemented in Phase 4 `src/pusher/client.rs` line 65; Phase 5 consumes already-deduplicated events |
| PRT-08 | 05-01-PLAN.md + 05-02-PLAN.md | Ack sent only after status written to SQLite | SATISFIED | C4 ordering enforced on both code paths; `update_precedes_ack_in_source` test statically asserts this |
| PRT-09 | 05-02-PLAN.md | Per-type enable/disable flag respected | SATISFIED | Disabled-type branch in `print_worker.rs` lines 90-109; `enabled_types_filter` test passes; fail-safe allow-all for empty/missing config |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `Cargo.toml` | 73 | `# TODO Phase 5: move to portable...` | INFO | Pre-existing comment from Phase 2/4 commits — NOT introduced by Phase 5 (confirmed via `git diff`). No formal issue reference, but this is a comment-only annotation on a working dependency, not a stub or blocker. |

**Debt marker gate assessment:** The `TODO` in `Cargo.toml` line 73 was present in commit `670e9e8` (Phase 4 era), before Phase 5's first commit `9070c9a`. Phase 5 did NOT introduce this marker. Per the debt-marker gate rule ("files modified by this phase"), this marker does not trigger a BLOCKER. The Phase 5 modification to `Cargo.toml` was only adding `base64 = "0.22"` (no new TODO/FIXME/XXX added).

No `TBD`, `FIXME`, or `XXX` markers found in any Phase 5 modified files.

### Human Verification Required

### 1. < 1 Second Print Latency (PRT-06 / SC-1)

**Test:** On a Windows machine with a connected thermal printer (e.g. Epson TM-T20X), trigger a pedido event in Noren and measure elapsed time from Pusher event receipt to paper coming out.
**Expected:** Comanda sai na impressora em menos de 1 segundo.
**Why human:** Requires physical Windows environment + thermal printer + live Pusher connection. No way to measure real I/O latency in CI.

### 2. Physical Print Correctness — Three Ticket Types (PRT-02/03/04 / SC-2)

**Test:** Trigger pedido, despacho, and fechamento events in the Noren staging environment. Inspect printed output physically.
**Expected:** Pedido ticket contains order items. Despacho ticket includes a QR code that scans as the dispatch token. Fechamento ticket contains closing totals. All match Noren's ESC/POS template output.
**Why human:** Requires visual inspection of printed output and a QR scanner. The agent is a "dumb spooler" — ESC/POS bytes come from Noren; correctness depends on Noren's rendering, not agent logic. But end-to-end output must be verified.

### 3. Dual Printer Path — USB Spooler and COM Serial (PRT-05 / SC-6)

**Test:** On Windows, configure the agent with a USB spooler printer (`PrinterId::Spooler`), trigger a print event, verify output. Then reconfigure with a COM port printer (`PrinterId::Serial`), repeat.
**Expected:** Both paths produce correct printout. RAW datatype used in spooler path (no rendering). COM port receives raw bytes directly.
**Why human:** Linux CI runs `StubPrinter` (no-op). Both paths require real Windows hardware. `StubPrinter::print_raw` always returns `Ok(())` — it doesn't prove the spooler or serial path works.

### 4. Job-Type String Cross-Check vs Noren Emit Code (D-06)

**Test:** In the Noren repo, grep the event-emission code (order/dispatch/closing transitions) for the exact string values in the `type` field of emitted Pusher events.
**Expected:** The strings Noren emits (e.g. `"order"`, `"dispatch"`, `"closing"`) exactly match the values that could appear in `enabled_types` config — no silent mis-routing due to string mismatch.
**Why human:** Requires cross-repo access to the Noren codebase. Cannot verify without that access.

---

### Gaps Summary

No automated gaps. All 9 must-have truths are VERIFIED in the codebase. The phase goal is structurally achieved: the full fetch→print→UPDATE→ack pipeline is implemented, wired, tested with 6 passing tests (including static C4 ordering and enabled_types filter verification), and the build is clean (40 tests, 1 ignored, 0 failures).

Status is `human_needed` because 4 items require physical hardware or cross-repo access to fully close (PRT-02/03/04/05/06 physical correctness + D-06 string cross-check). These were explicitly documented as "deferred to hardware verify" in the 05-02-SUMMARY.md and are expected at this stage of the project.

---

_Verified: 2026-07-16_
_Verifier: Claude (gsd-verifier)_
