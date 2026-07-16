# Phase 4: Pusher Event Stream - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-07-16
**Phase:** 04-pusher-event-stream
**Areas discussed:** Credential persistence, Event handoff to Phase 5, Dev testability, Reconnect aggressiveness

---

## Area Selection

| Option | Description | Selected |
|--------|-------------|----------|
| Credential persistence | `pusher_key`/`pusher_cluster` from activation: ConfigStore vs compile-time constants | ✓ |
| Event handoff to Phase 5 | mpsc channel vs SQLite insert for received events | ✓ |
| Dev testability | Fake Pusher shim for pre-Noren testing | ✓ |
| Reconnect aggressiveness | Backoff parameters, zombie threshold, tray color policy | ✓ |

**User's choice:** All four areas selected. User delegated: "pode decidir tudo e já criar o
contexto, não entendo muito sobre a parte técnica desse projeto, então vou ajustando nos testes
finais." — same posture as Phases 1 and 3 (full delegation to Claude).

---

## Claude's Discretion

All four gray areas were fully delegated. Decisions made:

- **Credential persistence** → ConfigStore (SQLite) — `pusher_key`, `pusher_cluster` stored at
  activation time. More flexible than compile-time constants (allows rotation/env changes).
- **Event handoff** → Hybrid: SQLite `INSERT OR IGNORE` into `printed_jobs` (status='pending')
  immediately on receive (C3 dedup fence), then `mpsc::Sender<PrintEvent>` for Phase 5. Crash-safe
  record + low-latency processing.
- **Dev testability** → `BREVLY_FAKE_PUSHER_EVENT=<jobId>:<type>` env var, gated by
  `cfg(debug_assertions)`. Bypasses real WS connection; emits synthetic event after 1 s. Compile
  out in `--release`.
- **Reconnect** → Ping 30s, zombie at 1 missed pong, backoff 1→2→4→8→16→32→60s cap with ±25% jitter,
  yellow tray indefinitely (never red from network failures — red reserved for printer hardware, Phase 6).

## Deferred Ideas

- Presence channel / online indicator for dashboard (future OBS-01)
- Pusher connection status broadcasting to Noren (future observability phase)
- Additional Pusher event types beyond `print-job` (handled gracefully by logging and ignoring)
