# Phase 3: Tray + Runtime + First Distributable - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-07-16
**Phase:** 03-tray-runtime-first-distributable
**Areas discussed:** Tray state source, Tray menu & quit policy, Single-instance guard, Installer signing scope

> **Owner posture:** For all four gray areas the owner replied *"Cara não entendo muito sobre
> isso, pode decidir tudo e criar o context"* — delegating every decision to Claude (same as
> Phase 1). The "Selected" column below reflects Claude's exercised recommendation, not an owner
> pick. Full rationale is in `03-CONTEXT.md`.

---

## Tray state source (RUN-02, before Pusher exists)

| Option | Description | Selected |
|--------|-------------|----------|
| Wire full state machine, stub signal | Build real HealthState (green/yellow/red) + tray-color plumbing now, fed by a placeholder 'healthy' signal; Phase 4 plugs Pusher in with no rework. Boots green once activated. | ✓ |
| Static placeholder until Phase 4 | Tray shows one fixed state now; tri-color logic deferred to Phase 4. Less code now, rework later. | |
| Tie to something real now | Drive color from what's available in Phase 3 (e.g. red if printer missing), not the connection signal RUN-02 ultimately means. | |

**Choice:** Wire full state machine, stub signal (delegated → Claude). Seed = green once startup
succeeds; state transitions arrive via a new `UserEvent::HealthChanged` on the event loop (D-01..D-04).
Kept the "printer missing → red at boot" idea as an optional best-effort signal (D-03).
**Notes:** Chosen so Phase 4 (Pusher) and Phase 6 (printer failure) feed the existing machine with
zero rework; honors pitfall C2 by keeping tray mutation on the loop thread.

---

## Tray menu & quit policy (RUN-01)

| Option | Description | Selected |
|--------|-------------|----------|
| Status + Reativar + Sobre, guarded Quit | Status line, 'Reativar impressora/licença' (reopens activation), 'Sobre/versão', and 'Sair' behind a confirmation. | ✓ |
| Minimal: status + Sair only | Just a status line and a plain 'Sair'. | |
| No quit at all | Status + Reativar + Sobre but no quit — only Task Manager can stop it. | |

**Choice:** Status + Reativar + Sobre, guarded Quit (delegated → Claude). "Sobre"/"Sair" use native
`MessageBoxW`; "Reativar" reopens the Phase 2 activation window on-demand (D-06/D-10).
**Notes:** Guarded quit copy states the consequence ("As impressões vão parar…"). Rejected no-quit
as trapping and unguarded quit as too easy to silently kill the agent.

---

## Single-instance guard

| Option | Description | Selected |
|--------|-------------|----------|
| Named mutex, second exits silently | First instance holds a Windows named mutex; second launch detects it and exits immediately. | ✓ |
| Second instance shows a toast then exits | Same guard but notifies before exiting. | |
| Defer to a later phase | Skip for now; revisit when double-execution becomes harmful (Phase 4/5). | |

**Choice:** Named mutex, second exits silently (delegated → Claude). `CreateMutexW` +
`ERROR_ALREADY_EXISTS` check placed right after the Velopack bootstrapper, before the runtime (D-08).
**Notes:** Toast rejected because it would pull `tauri-winrt-notification` forward from Phase 6 for
marginal benefit (D-09). Guard lands now (~10 lines) to prevent double-agent → double-print once
Pusher/print pipeline exist.

---

## Installer signing scope (DIST-01)

| Option | Description | Selected |
|--------|-------------|----------|
| Build pipeline now, gate real signing on cert | Full `vpk`/installer packaging + signtool CI step this phase; cert procurement is an explicit external blocker. | ✓ |
| Block phase until cert in hand | Don't call Phase 3 done until a real OV-signed installer exists. | |
| Self-signed / unsigned dev installer now | Working installer with a dev cert to test install→autostart→reboot, real OV signing tracked separately. | ✓ (blended) |

**Choice:** Build pipeline now + gate real OV signature on the cert, **and** produce a self-signed
dev installer now to prove the install→autostart→reboot→tray loop without waiting on procurement
(D-12/D-13). SmartScreen reputation timeline documented for the owner (D-14).
**Notes:** Blended options 1 and 3 — mirrors how Phase 2 completion is gated on the Noren backend:
planning + SC-1..SC-3 proceed now; SC-4 (signed, no "Unknown publisher") verifies once the OV cert
lands. Rejected fully blocking the phase (stalls SC-1..SC-3, which don't need signing).

---

## Claude's Discretion

The owner delegated **all four** areas. Additional technical calls made within that delegation
(planner may refine, defaults given): tray icon rendering (embedded PNG vs drawn RGBA circles, D-05);
in-loop window recreation vs exit-relaunch for "Reativar" (D-10, in-loop preferred); whether to
include boot-time printer-missing→red (D-03); mutex scope name + Win32 ergonomics (D-08); CI job
structure for `vpk pack` + conditional `signtool` (D-12); left/double-click tray behavior (D-07).

## Deferred Ideas

- Real green/yellow/red **connection** signal → Phase 4 (Pusher).
- Windows toast notifications (incl. failure/second-instance) → Phase 6 (RES-02).
- Auto-update download/apply + SHA256 verify (DIST-02/03) → Phase 7.
- Branded tray artwork (vs plain colored dots) → future UI-polish pass.
- Boot-crash job recovery (jobs stuck in `printing`) → Phase 6 (RES-04).
