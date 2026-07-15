# Phase 2: Activation - Context

**Gathered:** 2026-07-15
**Status:** Ready for planning

<domain>
## Phase Boundary

A one-time activation window (grown from the Phase 1 `winit`+`egui` spike) that lets a
restaurant owner bind this machine to their Noren tenant and get to a ready, autostarting agent:

1. Owner enters a **serial** → validated server-side by Noren (`POST /api/agent/activate`),
   binding this machine to a tenant and returning an `agentToken`.
2. Owner selects **one printer** from a combined list (Windows printers + COM ports).
3. Owner runs a **test-print** (visible receipt + cut) to confirm the hardware works.
4. On save: `agentToken` stored encrypted (DPAPI), config persisted (SQLite), **autostart
   registered (HKCU Run)**, window closes.
5. **Re-activation:** if the DPAPI credential later becomes unreadable (e.g., Windows reinstall),
   the agent re-enters the activation flow automatically.

Covers requirements **ACT-01 … ACT-08**.

**Out of scope (belongs to other phases):** tray icon + green/yellow/red runtime (Phase 3),
signed installer / SmartScreen (Phase 3, DIST-01), Pusher subscription (Phase 4), the real print
job pipeline / ESC/POS byte fetch / dedup / ack (Phase 5), printer-failure retry + toast (Phase 6).
An agent drives **one** printer in v1 (ACT-04, singular).
</domain>

<decisions>
## Implementation Decisions

### Serial input & validation
- **D-01:** **Serial is per-machine** — keep the existing Noren contract as-is (do NOT change the
  serial model). One serial ⇒ one machine; a multi-register restaurant uses multiple serials.
  Chosen explicitly to not disturb in-flight Noren work. (Rejected: per-tenant serial allowing N
  machines; per-tenant with a cap — both would change the `agent_serials` schema.)
- **D-02:** **409 "serial já ativo em outra máquina" → re-bind with confirmation.** Show
  "Este serial já está ativo em outro computador. Migrar a licença para esta máquina?"; on confirm,
  Noren invalidates the old token and issues a new one (same mechanism as the ACT-07 reinstall
  path). Covers legitimate PC swaps without a support ticket.
- **D-03:** Serial field UX is **Claude's discretion** — planner picks based on the real serial
  format Noren generates: segmented/masked input if the serial is fixed-length; a freeform field
  that accepts paste (Ctrl+V) if the format is variable. Confirm the actual format from Noren
  before implementing.
- **D-04:** **Validation feedback:** spinner on the "Ativar" button while calling Noren; success
  advances the flow; errors render **inline** (red text below the field) **without closing the
  window** (ACT-02/ACT-03).

### Printer selection & test-print
- **D-05:** **Single combined dropdown with a type label** per item, e.g. `EPSON TM-T20 (USB)` /
  `COM3 (Serial)`. The owner just sees "the printers"; no need to understand USB vs serial.
- **D-06:** **Pre-select the Windows default printer** (`get_default_printer`) when one exists —
  fewer clicks for the common single-printer case.
- **D-07:** **Empty state:** if NO Windows printer and NO COM port is detected, show
  "Nenhuma impressora encontrada — ligue a impressora e conecte o cabo, depois clique Atualizar"
  with a **"Atualizar lista"** button that re-enumerates in place. Save is **disabled** until a
  printer is available.
- **D-08:** **Test-print = visible receipt + mandatory step.** Prints a legible coupon (e.g.
  "Brevly Print — ativação OK", date/time) followed by the cut bytes (`ESC @` … `GS V`), and the
  owner confirms "funcionou?" as a required step of the flow (ACT-05).
- **D-09:** **Test-print failure does NOT hard-block save.** On a hardware failure (offline / out
  of paper), show "Não consegui imprimir — confira papel/cabo" but still allow completing
  activation — the real print path gets retry handling in Phase 6. (So: testing is a required
  *step*, but its *success* is not a save gate.)

### Re-activation (DPAPI loss, ACT-07)
- **D-10:** **Treat re-activation as a fresh activation — all fields blank.** Do NOT persist the
  serial to SQLite; nothing is pre-filled. Simplest and slightly more private. (Rejected:
  pre-filling serial+printer, which would require storing the serial in plaintext SQLite.)
- **D-11:** Detection UX (silent reopen vs a short "precisamos reativar este computador — sua
  licença continua válida" banner) is **Claude's discretion**, with the non-technical owner as the
  guiding norm (lean toward a short reassuring message so the returning window isn't alarming).

### Offline / backend unreachable
- **D-12:** **Distinguish network failure from an invalid serial.** Show
  "Sem conexão com o servidor — verifique a internet e tente de novo" for transport/connection
  errors vs "Serial inválido" for a 403/404 from Noren, with a "Tentar de novo" (manual retry)
  button. Full-offline activation is impossible (serial must be validated server-side to bind the
  tenant and mint the token) — so the requirement is clear messaging + manual retry, not a
  degraded mode.

### Autostart (ACT-08)
- **D-13:** **Register autostart (HKCU Run via `auto-launch`) at save time**, as part of completing
  activation — deterministic ("ativou → sobe com o Windows"). If the registry write fails, warn but
  still complete activation (autostart is recoverable later). (Rejected: registering only after the
  first successful Phase 3 runtime run — couples Phase 2 to Phase 3 and delays autostart.)

### Window layout
- **D-14:** **Single screen** — serial field + printer dropdown + test button + save button all on
  one screen (not a multi-step wizard). Short 3-field flow; less egui state to manage.

### Post-save lifecycle
- **D-15 (recommended default, planner may refine at the Phase 2/3 boundary):** after a successful
  save, **close the window and exit the process** (exit 0). Activation is a clean slice: token +
  config saved, autostart registered, process exits; the next launch/reboot comes up already
  activated and Phase 3 adds the tray/runtime. (Rejected for now: staying alive headless without a
  tray — mixes Phase 3 scope with no signal of life.)

### Claude's Discretion
- Serial field style (segmented vs freeform) — **D-03**, pin to the real Noren serial format.
- Re-activation banner copy/behavior — **D-11**.
- `machineId` generation/stability across reboots (technical) — planner decides; must be stable
  enough that a re-bind maps the same physical machine.
- Window sizing and egui widget styling (defer the *branding* to `/gsd:ui-phase 2`, see D-16).

### Branding
- **D-16:** **Delegate branding/visual design to `/gsd:ui-phase 2`** — logo, colors, window icon,
  language (PT-BR), typography get their own UI-SPEC.md. Phase 2 discussion stays focused on
  behavior. (Phase 2 carries "UI hint: yes" in the roadmap.)
</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Noren backend contract (cross-repo dependency — BLOCKS phase completion)
- `.planning/noren-contract-brief.md` §3 `POST /api/agent/activate` — request `{ serial, machineId? }`;
  response 200 `{ agentToken, tenantId, pusherKey, pusherCluster, enabledTypes }`; errors **404**
  serial inválido, **409** serial já ativo em outra máquina; **supports re-activation** (invalidates
  old token, mints new). This is the exact contract the activation flow codes against.
- `.planning/noren-contract-brief.md` §1 (`agent_serials` table: `serial` PK, `tenant_id`,
  `agent_token_hash`, `activated_at`, `revoked_at?`, `machine_id?`) — the server-side model behind
  D-01/D-02. **Blocker:** this table + `POST /api/agent/activate` must be live on Noren before
  Phase 2 can be verified/completed (planning may proceed now).

### Requirements
- `.planning/REQUIREMENTS.md` → **ACT-01 … ACT-08** (installer, first-run window, serial validation,
  combined printer list, test-print, DPAPI+SQLite persist, re-activation on DPAPI loss, autostart).

### Carried decisions & pitfalls
- `.planning/STATE.md` → "Key Decisions Locked" (egui raw over winit 0.30; agent is a dumb spooler;
  cross-platform core / Windows-only product; WritePrinter RAW not CreateFile) and pitfall **C1**
  (RAW datatype must be set on WritePrinter or ESC/POS becomes silent garbage — validate in the
  test-print, D-08).
- `.planning/phases/01-foundation-thread-model-spike/SKELETON.md` — architectural decisions the
  activation window inherits (event loop, store traits, cfg-gating, migration pattern).
- `.planning/ROADMAP.md` → Phase 2 section (Goal + 6 Success Criteria + ACT coverage).

### Stack references (from CLAUDE.md)
- `printers` 0.2.x (`get_printers()` + `get_default_printer()`) — Windows printer enumeration for
  the dropdown (D-05/D-06). `serialport` 4.7.x — COM port enumeration + raw write. `windows` 0.62.x
  `Win32::Graphics::Printing` (`OpenPrinterW`/`StartDocPrinterW` **with RAW datatype**/`WritePrinter`)
  for the spooler path. `auto-launch` 0.5.x (HKCU Run) for autostart (D-13). All under
  `[target.'cfg(windows)'.dependencies]` per the Phase 1 cross-platform split.
</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets (from Phase 1)
- `src/spike_window.rs` — the raw `egui` (`egui-winit` + `egui-wgpu`) render integration + the
  interactive frame (text field + button → label). **This is the direct ancestor of the activation
  screen** (D-14 single screen). Grow it into the serial field + printer dropdown + test/save buttons.
- `src/config_store.rs` — `ConfigStore` with migration v1 (tables `config`, `printed_jobs`,
  `retry_queue`) + `get`/`set` (upsert). Use `set` to persist the selected printer + tenant config
  on save (D-15); no schema change needed for config KV.
- `src/credential_store/` — `CredentialStore` trait + `CredentialError` (NotFound/Corrupt/Io),
  `DpapiCredentialStore` (Windows, `Scope::User`) / `DevFileCredentialStore` (Linux dev-only). Store
  the `agentToken` here on save; the **typed NotFound/Corrupt error is exactly the ACT-07
  re-activation trigger** (D-10) — startup reads the credential, and a NotFound/Corrupt result routes
  into the activation flow instead of the runtime.
- `src/app_dir.rs` — per-platform app dir (`dirs::data_dir()/BrevlyPrint/`, `create_dir_all`).
- `src/main.rs` — `winit 0.30` `ApplicationHandler` event loop + startup wiring; the branch point
  where "credential present → runtime / credential missing-or-corrupt → activation window" lives.

### Established Patterns
- **cfg-gated trait + two impls** (CredentialStore) is the precedent for a new **`Printer` trait**
  (Windows `WritePrinter` RAW impl / serial impl; a Linux dev stub) needed for the test-print (D-08)
  and later Phase 5. Follow the same `#[cfg(windows)]` / `#[cfg(not(windows))]` split.
- **`rusqlite_migration` versioned migrations** — if activation needs any new persisted field beyond
  the KV `config` table, add a migration v2 (never edit v1).
- Windows-only crates live under `[target.'cfg(windows)'.dependencies]`; the portable core still
  builds/tests on Linux. Printer enumeration + DPAPI + auto-launch are Windows-only; the Linux dev
  build stubs them so the activation *logic* stays testable on Linux.

### Integration Points
- Startup credential check (`src/main.rs`) → decides activation-window vs runtime.
- HTTP call to Noren `POST /api/agent/activate` — first network code in the project; use `reqwest`
  (rustls) per the stack. Handle transport-error vs HTTP-status distinctly (D-12).
- `auto-launch` HKCU registration invoked on save (D-13).
</code_context>

<specifics>
## Specific Ideas

- Test-print content should be a **legible** coupon the owner can read ("Brevly Print — ativação OK"
  + date/time), not just silent cut bytes — the owner must *see* proof, not just hear the cutter
  (D-08).
- Error copy should be plain-language and blame-clarifying: network problem ("verifique a internet")
  vs serial problem ("Serial inválido") are worded differently so the owner knows whose fault it is
  (D-12).
</specifics>

<deferred>
## Deferred Ideas

- **Per-tenant serial / multi-machine licensing model** — considered (D-01) but explicitly kept as
  per-machine to match the current Noren contract. If Noren later wants one serial to cover multiple
  registers, that's a licensing-model change (Noren-side schema + a roadmap revisit), not this phase.
- **One agent driving multiple printers** — out of scope for v1 (agent = one printer, ACT-04). Would
  be its own capability/phase.
- **Branding / visual identity** — routed to `/gsd:ui-phase 2` (D-16), not lost.
- **Tray icon / runtime / signed installer** — Phase 3. **Print retry + toast on failure** — Phase 6.

None of the above expanded Phase 2 scope — discussion stayed within the activation boundary.
</deferred>

---

*Phase: 02-activation*
*Context gathered: 2026-07-15*
