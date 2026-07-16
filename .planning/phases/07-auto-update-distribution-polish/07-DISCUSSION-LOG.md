# Phase 7: Auto-Update + Distribution Polish - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-07-16
**Phase:** 7-auto-update-distribution-polish
**Areas discussed:** Update source contract, SHA256 verification ownership, "Update ready" notification, Scope of "Distribution Polish"

---

## Which areas to discuss

| Option | Description | Selected |
|--------|-------------|----------|
| Update source contract | Velopack-native feed vs. custom `GET /api/agent/version` endpoint; decides what Noren hosts | |
| SHA256 verification ownership | Explicit manual check vs. rely on Velopack's built-in integrity | |
| "Update ready" notification | Windows toast vs. tray tooltip/menu line vs. silent | |
| Scope of "Distribution Polish" | Auto-update only vs. also OV-signing gate + release/versioning process | |

**User's choice:** "pode decidir tudo" (decide everything) — owner delegated all four gray areas, same posture as Phases 1 and 3.
**Notes:** Owner consistently defers implementation decisions on distribution/update infrastructure. All four areas resolved at Claude's discretion and locked in CONTEXT.md (D-01 through D-07).

---

## Claude's Discretion

All decisions were exercised under delegation. Locked:
- **D-01** — Noren `/api/agent/version` authoritative for version; Velopack remains the apply mechanism (safe locked-exe swap on next boot). Exact Velopack Rust API is a research item; feed-directory fallback noted.
- **D-02** — Explicit manual SHA256 check is the authoritative DIST-03 gate; mismatch aborts without touching the running agent. Add `sha2` (RustCrypto), pure Linux-testable function.
- **D-03** — Check on startup + poll ~6h; off the print critical path; silent on failure.
- **D-04** — Quiet two-tier "update ready" signal: persistent tray status line + one-shot Phase 6 toast. Tray icon color stays reserved for connection health.
- **D-05** — Apply strictly on next natural reboot/login via the already-wired Velopack bootstrapper; no forced restart.
- **D-06** — "Polish" = close the release/publish loop (version-bump discipline + CI produces/publishes the update package + surfaces `version`/`downloadUrl`/`sha256`). OV signing stays an external gate; branded icons deferred.
- **D-07** — `#[cfg(windows)]` gate the Velopack apply + toast; keep version-compare + SHA256-verify pure and Linux-tested.

Remaining fine-grained discretion left to planner/executor: exact Velopack API path, whether the version endpoint needs bearer auth, poll interval tuning, PT-BR copy, `sha2` version, CI job structure.

## Deferred Ideas

- Immediate / idle-time self-restart to apply updates sooner (v1 applies on next reboot only).
- OV certificate procurement + SmartScreen reputation warm-up (external, carried from Phase 3).
- Branded tray artwork (deferred from Phase 3 D-05).
- Staged/percentage rollouts or a Noren-side kill-switch for a bad release (Noren backlog).
- Update channels (beta/stable) — single stable channel in v1.
