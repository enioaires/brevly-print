# Phase 3: Tray + Runtime + First Distributable - Context

**Gathered:** 2026-07-16
**Status:** Ready for planning
**Note:** Owner delegated all decisions on the four gray areas ("Cara não entendo muito sobre
isso, pode decidir tudo e criar o context") — same posture as Phase 1. Every decision below is
**locked** (Claude's discretion, exercised). Downstream agents should treat these as decided,
not open. Where a call is genuinely a technical implementation detail, it is flagged for the
planner but still given a recommended default so nothing stalls.

<domain>
## Phase Boundary

Turn the app from a one-shot **activation window** into an always-on, **invisible tray agent**.
Today `src/main.rs` builds everything up to the credential check, and when a credential exists it
just prints `"Phase 3: start tray runtime here"` and exits. This phase makes that branch a real
**headless runtime**:

1. **Runtime path** — activated launch (credential present) starts a long-lived process with **no
   visible window**: a `tray-icon` in the system tray driven by the existing `winit 0.30`
   `ApplicationHandler` event loop. No taskbar entry, no pop-up (RUN-01).
2. **Tray state machine** — the tray icon reflects a `HealthState` (green = healthy, yellow =
   reconnecting, red = connection/printer problem). The full tri-color plumbing is wired now; the
   real signal source (Pusher) arrives in Phase 4 (RUN-02).
3. **Right-click menu** — status line + "Reativar" + "Sobre/versão" + a confirmed "Sair" (RUN-01).
4. **Survives reboot** — autostart (registered in Phase 2, HKCU Run, D-13) launches the runtime
   path on login; the tray appears with no user action (RUN-03).
5. **First distributable** — package the release into a Windows installer via Velopack `vpk` and
   wire an Authenticode `signtool` signing step into CI, gated on the OV certificate (DIST-01).

Covers requirements **RUN-01, RUN-02, RUN-03, DIST-01**.

**Out of scope (belongs to other phases):**
- Pusher/WebSocket subscription and the *real* connection signal that drives green/yellow/red —
  **Phase 4** (this phase seeds the state as healthy and exposes the plumbing).
- Print job pipeline: ESC/POS byte fetch, WritePrinter/serial, dedup, ack — **Phase 5**.
- Printer-failure retry + Windows **toast** notifications + boot-crash job recovery (RES-04) —
  **Phase 6** (this phase only proves the *process* survives reboot, not that in-flight jobs do).
- Auto-update download/apply flow (DIST-02/03) — **Phase 7** (Velopack bootstrapper is already the
  first call in `main`; this phase reuses `vpk` only for *packaging*, not the update loop).
</domain>

<decisions>
## Implementation Decisions

### Tray state machine (RUN-02) — GA1
- **D-01:** **Wire the full `HealthState` machine now, seed it with a stub "healthy" signal.**
  Define `HealthState { Connected /*green*/, Reconnecting /*yellow*/, Problem /*red*/ }` and the
  tray-color swap plumbing this phase. Phase 4 (Pusher) and Phase 6 (printer failure) feed real
  transitions into it with **zero rework**. Rejected: a static single-color placeholder (would
  need the tri-color logic rebuilt in Phase 4) and driving color off printer-reachability only
  (a real but *wrong* signal — RUN-02 ultimately means the *connection* state).
- **D-02:** **Seed state = `Connected` (green) once startup succeeds** (credential ok, stores
  open, tray created). This is the honest Phase-3 truth: the agent is up and healthy; it just has
  nothing to be connected *to* yet. Phase 4 will start in `Reconnecting` (yellow) until the Pusher
  handshake completes, then flip to `Connected`.
- **D-03 (optional real signal — planner's call):** If cheap, also set `Problem` (red) at boot when
  the printer named in `config` is absent from enumeration — a genuine, available-now signal that
  partially satisfies SC-2 ("red when a printer problem is detected"). Keep it best-effort and
  non-blocking; do **not** let a slow/flaky printer probe delay tray appearance. If it adds
  meaningful complexity, skip it — the stub-green seed (D-02) already satisfies Phase 3.
- **D-04:** **State transitions arrive via the winit event loop, not shared locks.** Extend the
  existing `UserEvent` enum (already stubbed in `main.rs` for `TrayIconEvent`/`MenuEvent`) with a
  `HealthChanged(HealthState)` variant. Background tasks (Phase 4+) push health changes through an
  `EventLoopProxy`; the `user_event` handler swaps the tray icon. Keeps all tray mutation on the
  event-loop thread (avoids the C2-class threading hazards the whole project is built to avoid).

### Tray icons (visual assets) — GA1
- **D-05:** **Minimal solid-color status icons** (green / yellow / red circle/dot), embedded in the
  binary via `include_bytes!` and loaded as `tray_icon::Icon` RGBA. No `/gsd:ui-phase 3` pass —
  Phase 3 does **not** carry a "UI hint" in the roadmap, and functional colored dots meet RUN-02.
  Branded tray artwork is a deferred polish item (owner may request a UI pass later). Planner:
  keep the three icons dimensioned per Windows tray expectations (16×16 / 32×32 source).

### Tray right-click menu & quit policy (RUN-01) — GA2
- **D-06:** **Menu = status line + "Reativar impressora/licença" + "Sobre" + "Sair" (confirmed).**
  - **Status line** — a **disabled** menu item showing the current health in plain PT-BR
    ("Conectado" / "Reconectando…" / "Problema de conexão"), updated as `HealthState` changes.
  - **"Reativar impressora/licença"** — reopens the **activation window on-demand** inside the
    already-running event loop (reuses Phase 2's `ActivationWindow`). Lets the owner change printer
    or re-bind the serial without reinstalling. See D-10 for the lifecycle mechanics.
  - **"Sobre"** — shows version (`env!("CARGO_PKG_VERSION")`) + product name in a native Win32
    `MessageBoxW` (no egui window needed for a one-line info dialog).
  - **"Sair"** — **guarded**: a `MessageBoxW` confirm ("Fechar o Brevly Print? As impressões vão
    parar enquanto o programa estiver fechado.") Yes → clean `event_loop.exit()`. Rejected: no-quit
    (feels trapping; owner is the admin of their own machine) and an unguarded quit (too easy to
    silently kill the printer agent).
- **D-07:** **Left-click / double-click on the tray icon = no-op in Phase 3** (or, planner's
  discretion, mirror the "Sobre" info). All real actions live in the right-click menu. Keeps
  behavior predictable and avoids accidental window pop-ups that would violate RUN-01.

### Single-instance guard — GA3
- **D-08:** **Enforce single instance via a Windows named mutex; the second launch exits silently.**
  Create `CreateMutexW` with a stable per-user name (e.g. `Local\\BrevlyPrintAgent` — `Local\` =
  per-session, correct for a per-user tray app launched from HKCU autostart). If
  `GetLastError() == ERROR_ALREADY_EXISTS`, the process exits `0` immediately. This is the fence
  against **autostart + manual double-click (or a reboot race)** spawning two agents → two Pusher
  subscriptions → **double prints** once Phase 4/5 land. Rejected: deferring the guard (the risk is
  real and the fix is ~10 lines; cheaper to land now while the runtime is being built).
- **D-09:** **Silent exit, not a toast.** A toast on the second launch would pull
  `tauri-winrt-notification` infrastructure forward from Phase 6 for marginal benefit. Silent exit
  is the standard behavior and keeps the "invisible agent" promise. (If the owner later wants
  double-click feedback, revisit in Phase 6 when toast infra exists.) Place the mutex check **very
  early** in `main()` — after the Velopack bootstrapper (which must stay the first call, OQ3),
  before building the tokio runtime and event loop.

### Runtime lifecycle & `main.rs` unification — GA1/GA2 (technical, planner-facing)
- **D-10:** **Unify the two startup paths under one event loop.** Today `main.rs` branches *before*
  building the event loop (activation → window; activated → exit). Phase 3 restructures so the
  event loop is **always** built, and `App` carries a mode:
  - `needs_activation == true` → **Activation mode**: create `ActivationWindow` in `resumed()`
    (unchanged Phase 2 behavior). On save, Phase 2 currently exits the process (D-15); that stays —
    the next launch/reboot comes up already-activated into Runtime mode.
  - `needs_activation == false` → **Runtime mode**: create the **`TrayIcon`** in `resumed()` (per
    the `tray-icon` + winit official example — tray created after the loop starts), **no window**.
    winit 0.30 runs an event loop with zero windows fine on Windows; the loop still pumps the Win32
    messages `tray-icon` needs. `ControlFlow::Wait` (event-driven; the runtime is idle until a
    tray/menu/health event arrives — no busy redraw loop like the activation window uses).
  - **Reativar** transitions Runtime → a transient window: create an `ActivationWindow` on demand
    while the tray keeps running; on completion, dispose the window and return to pure tray. Planner
    may keep this simple by reusing the exit-and-relaunch pattern if in-loop window creation proves
    fiddly, but in-loop is preferred (no flash of the process disappearing).
- **D-11:** Keep the **tokio runtime alive for the whole process** (already the case) — the runtime
  path will need it for Pusher (Phase 4). Tray/menu event forwarding uses `event_loop.create_proxy()`
  at the marked spot in `main.rs`.

### First distributable & signing (DIST-01) — GA4
- **D-12:** **Build the full packaging + signing *pipeline* this phase; gate the *real* OV signature
  on the certificate as an explicit external blocker.** Concretely:
  - Use **Velopack `vpk pack`** to produce the Windows `Setup.exe` from the release binary
    (consistent with the Phase 7 auto-update toolchain — same tool builds installer and update).
  - Wire a **`signtool sign`** step into the Windows CI job (GitHub Actions), reading the cert from a
    CI secret and **skipping cleanly when the secret is absent** so builds don't fail pre-cert.
  - The **OV certificate procurement is the external blocker** for final DIST-01 sign-off — mirrors
    how Phase 2's completion is gated on the Noren backend. Planning and all other Phase 3 work
    proceed now; SC-4 (signed, no "Unknown publisher") is verified once the cert lands.
- **D-13:** **Produce a self-signed / unsigned dev installer now** to prove the install → autostart
  → reboot → tray-appears loop (SC-1, SC-2, SC-3) **without waiting on procurement**. The self-signed
  build is dev/test-only and MUST NOT be presented as the shippable artifact. Real OV signing is
  tracked as the DIST-01 completion gate (D-12). Rejected: blocking the whole phase until the cert
  is in hand (stalls SC-1..SC-3, which don't need signing) and shipping unsigned to real owners
  (Windows blocks it — see CLAUDE.md signing notes).
- **D-14:** **Document the SmartScreen reputation reality** in the phase artifacts: even a correctly
  OV-signed installer shows warnings until download-volume reputation builds (~weeks; the existing
  STATE todo "Plan SmartScreen reputation period (2–6 weeks after first signed release)"). This is a
  *timeline* expectation for the owner, not a code task — SC-4's "does not block/warn Unknown
  publisher" is about the **publisher identity** (satisfied by a valid OV cert), separate from the
  reputation-warmup curve.

### Claude's Discretion (delegated — planner/executor finalize)
- Exact tray icon rendering (embedded PNG vs programmatically-drawn RGBA circles) — D-05.
- In-loop window recreation vs exit-relaunch for "Reativar" — D-10 (in-loop preferred).
- Whether to include the boot-time printer-missing → red signal — D-03 (best-effort, skip if costly).
- Mutex scope name and exact Win32 call ergonomics (`windows` crate) — D-08.
- CI job structure for `vpk pack` + conditional `signtool` — D-12.
- Left/double-click tray behavior — D-07.
</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase requirements & grading
- `.planning/ROADMAP.md` → **Phase 3: Tray + Runtime + First Distributable** — Goal + the **4
  Success Criteria** this phase is graded on (reboot→tray, tri-color, no-window, signed installer).
- `.planning/REQUIREMENTS.md` → **RUN-01, RUN-02, RUN-03, DIST-01** (invisible tray-only runtime;
  green/yellow/red state; survives reboot & reconnects; Authenticode-signed installer). DIST-02/03
  (auto-update) are **Phase 7**, not here.

### The runtime branch point (what this phase grows)
- `src/main.rs` — the current startup order + the **credential branch** where
  `needs_activation == false` prints "Phase 3: start tray runtime here" and exits. The `UserEvent`
  enum, `App` `ApplicationHandler`, `ControlFlow::Wait`, and the `create_proxy()` wiring spot are
  all stubbed here for Phase 3 (see the `// Phase 3:` comments). This is the primary file to
  restructure (D-10).
- `src/activation_window.rs` + `src/activation_state.rs` — Phase 2's `ActivationWindow`, reused
  on-demand by the tray "Reativar" menu (D-06/D-10).

### Carried decisions, pitfalls & architecture
- `.planning/STATE.md` → "Accumulated Context": locked decisions (egui raw over winit 0.30; agent =
  dumb spooler; cross-platform core / Windows-only product; Velopack for update) and pitfalls,
  esp. **C2** (event-loop conflict — why all tray/window mutation stays on the loop thread, D-04)
  and **C5** (Pusher zombie — relevant to the yellow "Reconnecting" state Phase 4 will drive).
  Also the open todos: **"Procure Authenticode OV certificate (needed for Phase 3 distributable)"**
  and **"Plan SmartScreen reputation period"** (D-12/D-14).
- `.planning/phases/01-foundation-thread-model-spike/01-CONTEXT.md` + `SKELETON.md` — the event-loop
  + cfg-gating architecture the runtime inherits; `01-RESEARCH.md` §Standard Stack documents the
  **`tray-icon 0.24` + `winit 0.30`** integration (official `winit.rs` example) that the tray path
  follows.
- `.planning/phases/02-activation/02-CONTEXT.md` → **D-13** (autostart already registered via
  `auto-launch` HKCU Run at save — RUN-03 depends on this being present) and **D-15** (activation
  exits the process on save → next launch lands in *this* runtime).

### Stack references (from CLAUDE.md)
- `tray-icon` 0.24 — tray icon, tooltip, right-click menu, color-state (D-01/D-05/D-06); Windows-only
  under `[target.'cfg(windows)'.dependencies]`.
- `auto-launch` 0.6 (HKCU Run) — already used in Phase 2; RUN-03 relies on it (no new work expected,
  just verify the autostart command launches the installed exe into Runtime mode).
- `windows` 0.62 — `CreateMutexW`/`GetLastError` for the single-instance guard (D-08) and
  `MessageBoxW` for the "Sobre" / "Sair" confirm dialogs (D-06). May require adding the
  `Win32_System_Threading` and/or `Win32_UI_WindowsAndMessaging` feature flags to the existing
  `windows` dep — planner to confirm.
- `velopack` + **`vpk` CLI** 1.2.x — build the `Setup.exe` (D-12); same toolchain as the Phase 7
  auto-update flow. `signtool.exe` (Windows SDK) — Authenticode signing step in CI (D-12).
- CLAUDE.md → "Windows Installer + Code Signing" and "Confidence Assessment (Installer + signing:
  MEDIUM — EV no longer instant-clean; plan reputation-building time)" — the basis for D-14.
</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/main.rs` — the `winit 0.30` `ApplicationHandler` (`App`) already exists with the
  `UserEvent` enum, `user_event()` handler, `about_to_wait()` redraw pump, and a persistent tokio
  runtime + shared `reqwest::Client`. Phase 3 adds Runtime mode alongside the existing Activation
  mode rather than writing a new loop. The `// Phase 3:` comments mark every extension point
  (`TrayIconEvent`/`MenuEvent` variants, `create_proxy()`, "start tray runtime here").
- `src/activation_window.rs` / `src/activation_state.rs` — the full activation window, reused by the
  tray "Reativar" action (D-06). No changes to its internals expected — just an on-demand
  construction path from Runtime mode.
- `src/config_store.rs` — `ConfigStore` KV (`printer_name`, `printer_type`, `tenant_id`,
  `enabled_types`). Runtime reads `printer_name`/`printer_type` at boot for the optional
  printer-present health check (D-03) and, later, for the Phase 5 print path.
- `src/credential_store/` — the startup credential probe that already decides Activation vs Runtime
  is the exact branch Phase 3 fills in.

### Established Patterns
- **All GUI/tray state mutated on the event-loop thread**, background work talks to it via
  `EventLoopProxy` + `UserEvent` (Pattern from Phase 1, enforced by pitfall C2). D-04's
  `HealthChanged` variant follows this precisely.
- **`#[cfg(windows)]` / `#[cfg(not(windows))]` gating** — the tray, mutex, `MessageBoxW`, and
  `vpk`/signing are all Windows-only. The portable core still builds/tests on Linux; the runtime
  path's Windows-only pieces are gated, with a Linux dev fallback (e.g. a no-tray "run headless and
  log state" stub) so the *logic* around health-state transitions stays testable on Linux.
- **Velopack bootstrapper stays the first call in `main()`** (OQ3) — the single-instance mutex check
  goes immediately after it, before the runtime/event loop.

### Integration Points
- `main.rs` credential branch → Runtime mode (tray) instead of the current exit stub.
- `tray-icon` event forwarding via `event_loop.create_proxy()` → `UserEvent::TrayIconEvent` /
  `UserEvent::MenuEvent` → menu action handlers (D-06).
- CI (GitHub Actions Windows job, established in Phase 1 D-03/D-23) gains a `vpk pack` step and a
  conditional `signtool` step (D-12) — the first *packaging* work in the project (Phase 1 CI was
  build+test only).
- `auto-launch` HKCU Run entry (written in Phase 2) → the OS relaunches the exe on login → Runtime
  mode (RUN-03). Verify the registered command points at the Velopack-installed exe path.
</code_context>

<specifics>
## Specific Ideas

- The three health states have fixed PT-BR labels for the tray status line and tooltip:
  **green = "Conectado"**, **yellow = "Reconectando…"**, **red = "Problema de conexão"** (D-06). Keep
  the wording plain and non-technical — the audience is a restaurant owner, not an admin.
- The "Sair" confirmation copy makes the consequence explicit: *"As impressões vão parar enquanto o
  programa estiver fechado."* — so the owner understands quitting stops printing (D-06).
- Phase 3 proves the process **survives reboot** (RUN-03); it does **not** yet prove that in-flight
  print jobs survive a crash — that's RES-04 in Phase 6. Keep the reboot-survival test scoped to
  "reboot → tray icon appears, no user action", not job recovery.
</specifics>

<deferred>
## Deferred Ideas

- **Real green/yellow/red *connection* signal** — driven by the Pusher handshake/ping-pong in
  **Phase 4**. Phase 3 wires the machine and seeds green (D-01/D-02); Phase 4 supplies the truth.
- **Windows toast notifications** (incl. any second-instance or failure toast) — **Phase 6** (RES-02),
  where `tauri-winrt-notification` infra is introduced. Not pulled forward for the single-instance
  guard (D-09).
- **Auto-update download/apply + SHA256 verify** (DIST-02/03) — **Phase 7**. Phase 3 reuses `vpk`
  only for packaging, and the Velopack bootstrapper is already wired.
- **Branded tray artwork** (logo-styled icons vs plain colored dots) — a future UI-polish pass if the
  owner wants it; functional colored dots ship in Phase 3 (D-05).
- **Boot-crash job recovery** (jobs stuck in `printing`) — **Phase 6** (RES-04).

None of the above expanded Phase 3 scope — discussion stayed within the tray/runtime/distributable
boundary.
</deferred>

---

*Phase: 03-tray-runtime-first-distributable*
*Context gathered: 2026-07-16*
