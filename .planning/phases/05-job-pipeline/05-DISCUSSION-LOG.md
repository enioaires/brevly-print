# Phase 5: Job Pipeline - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-07-16
**Phase:** 05-job-pipeline
**Areas discussed:** All areas (Claude's discretion — owner delegated)

---

## Gray Area Selection

| Option | Description | Selected |
|--------|-------------|----------|
| Job type strings | What exact values does Noren send as job_type? Affects PRT-09 filter | |
| Ack failure policy | What Phase 5 does when ack POST fails after successful print | |
| Schema: 'printing' state | Add migration for 'printing' now vs defer to Phase 6 | |
| Decide everything | Same posture as Phases 1, 3, and 4 — delegate all decisions to Claude | ✓ |

**User's choice:** "Decide everything" — owner delegated all Phase 5 decisions to Claude.
**Notes:** Consistent with prior phases. Owner stated explicitly in Phase 4: "pode decidir tudo
e já criar o contexto, não entendo muito sobre a parte técnica."

---

## Claude's Discretion

All Phase 5 decisions were made by Claude. Key choices exercised:

- **Print worker module** (`src/print_worker.rs`) mirrors `run_pusher_loop()` pattern from Phase 4.
- **`App._print_rx` removal** — the Phase 4 placeholder is removed; Phase 5 consumes `print_rx` directly at spawn time.
- **`enabled_types` filter** — disabled job types are marked `'printed'` + acked (not errored, not left `'pending'`).
- **Status: no `'printing'` in Phase 5** — deferred to Phase 6 (RES-04 crash recovery).
- **Ack failure: log + leave `'printed'`** — rely on Phase 6 pending pull for recovery; no inline retry.
- **`base64 = "0.22"`** added as new portable dependency for ESC/POS byte decoding.
- **Job type strings** documented as likely `"order"`, `"dispatch"`, `"closing"`, `"cancel"` — flagged for verification against Noren backend before Phase 5 ships.

## Deferred Ideas

- `status='printing'` intermediate state for crash recovery → Phase 6 (RES-04)
- Inline ack retry → Phase 6 pending pull covers it
- Typed fetch-failure errors (404 vs transport) → Phase 6 retry semantics
- Windows toast on print failure → Phase 6 (RES-02)
