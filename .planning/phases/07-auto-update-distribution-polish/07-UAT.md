# Phase 07: Auto-Update + Distribution Polish — UAT Checklist

**Created:** 2026-07-16
**Phase:** 07-auto-update-distribution-polish
**Agent:** 07-02 executor (Linux session — Windows hardware not available)

This file tracks manual Windows-hardware verification items that cannot be automated in a Linux
dev session. Each item was generated because the corresponding plan task was a
`checkpoint:human-verify` gate that cannot be executed without a real Velopack-installed binary
running on Windows.

---

## PENDING UAT Items

### UAT-07-01: OQ-1 Staging Persistence Spike

**Status:** PENDING — Windows hardware required
**From:** 07-02-PLAN.md Task 1 (`checkpoint:human-verify`)
**Design decision applied:** Design A (immediate call to `wait_exit_then_apply_updates`) per
RESEARCH.md recommended default. If the spike result is Design B, apply.rs must be updated to
defer `wait_exit_then_apply_updates` to the "Sair" handler / process exit path.

**Hardware required:** Windows machine/VM with Velopack-installed `BrevlyPrint` binary

**What to do:**

1. Build the agent, run `vpk pack --packId BrevlyPrint`, install v0.1.0 via the produced `Setup.exe`.
2. Publish a local Velopack feed for v0.1.1 (`releases.win.json` + full `.nupkg`) reachable by `HttpSource`.
3. Trigger the update check (either wait 10s for the background task, or modify `tokio::time::sleep` duration temporarily).
4. After `download_updates()` + `wait_exit_then_apply_updates(&update.to_apply, true, false, [])` is called,
   keep the process alive for **more than 90 seconds** (do NOT exit) so the updater's 60s timeout elapses.
5. Inspect `%LocalAppData%\BrevlyPrint\packages\` — is the staged `.nupkg` still present after 90s?
6. Exit the agent and relaunch. Does `VelopackApp::build().run()` apply the staged update (agent now v0.1.1)?

**Success criterion:**
- If YES (stage persists): Design A confirmed. `apply.rs` is correct as implemented.
- If NO (stage lost after 60s): Design B required. Update `apply.rs`:
  - `stage_update()` should call only `download_updates()`, then return `Ok(())`.
  - Add `pub fn apply_staged_on_exit() -> anyhow::Result<()>` that calls `wait_exit_then_apply_updates`.
  - Wire `apply_staged_on_exit()` from the "Sair" menu handler in `main.rs::handle_menu_event`.

**Also confirm:** The exact `UpdateInfo` field name passed to `wait_exit_then_apply_updates`.
The code uses `update.to_apply` (RESEARCH.md A2 — ASSUMED). If the field name is different
(check `docs.rs/velopack/1.2.0/velopack/struct.UpdateInfo.html`), update `apply.rs` accordingly.

**OQ-1 comment in code:** `src/update/apply.rs` contains:
```
// OQ-1: staging-persistence past the 60s updater timeout is UNVERIFIED on Linux — confirm on Windows (see 07-02 UAT).
```

---

### UAT-07-02: Windows End-to-End Update Verification (SC-1 / SC-2 / SC-3)

**Status:** PENDING — Windows hardware required
**From:** 07-02-PLAN.md Task 4 (`checkpoint:human-verify`)

**Hardware required:** Windows machine with Velopack-installed v0.1.0 + thermal printer + reachable `/api/agent/version`

**What to do:**

**SC-1 test (no print interruption, one toast, no icon change):**
1. Point the agent at a `/api/agent/version` endpoint serving `{ version: "0.1.1", downloadUrl: "<url>", sha256: "<correct_sha256>" }`.
2. While the (throttled) download runs, trigger a print event from Noren.
3. Confirm the comanda prints in < 1 second and the tray icon color does NOT change (stays green/yellow/red per health state).
4. Confirm the tray status line shows `"Atualização pronta — será aplicada ao reiniciar"`.
5. Confirm exactly ONE toast notification fired ("Atualização pronta. Será aplicada no próximo reinício.").
6. Wait for the 6-hour re-poll — confirm no second toast fires.

**SC-2 test (SHA256 mismatch aborts cleanly):**
1. Restart the agent, now serve a `sha256` that does NOT match the hosted `.nupkg` (e.g., all zeros).
2. Confirm NO tray status change, NO toast.
3. Relaunch the agent — confirm it comes up as v0.1.0 (Sobre dialog shows 0.1.0).

**SC-3 test (new version after reboot):**
1. With the correct sha256 case, let the update stage (tray shows "Atualização pronta").
2. Reboot or relaunch the agent naturally.
3. Confirm the agent comes up as v0.1.1 (Sobre dialog shows 0.1.1) with zero manual action.

**Success criterion:** All three pass (SC-1, SC-2, SC-3).

---

## COMPLETED UAT Items

*(none yet — all UAT pending Windows hardware)*

---

*This file is maintained by the 07-02 executor and updated by human verification sessions.*
