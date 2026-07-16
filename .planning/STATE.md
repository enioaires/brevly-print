---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
current_phase: 4
current_plan: Not started
status: planning
last_updated: "2026-07-16T15:00:15.768Z"
progress:
  total_phases: 7
  completed_phases: 3
  total_plans: 9
  completed_plans: 9
  percent: 43
---

# State: Brevly Print

## Project Reference

**Core Value:** Quando um evento de impressão chega do Noren, a comanda/cupom correto sai na impressora
térmica em menos de 1 segundo, de forma confiável e sem intervenção humana — nenhuma
comanda perdida, mesmo com impressora ou internet fora do ar.

**Stack:** Rust nativo (Windows-only) — tao + tray-icon + egui (raw) + tokio + reqwest (rustls) + rusqlite (bundled) + windows crate (WritePrinter RAW) + serialport + auto-launch + velopack + tauri-winrt-notification + windows-dpapi

**Cross-repo dependency:** Noren backend (`~/repos/brevly/noren`) must implement `/api/agent/*` contract before phases 2, 4, 5, 6, 7 can be completed. Server-side ESC/POS rendering is the longest-lead Noren item — gates Phase 5.

## Current Position

Phase: 03 (tray-runtime-first-distributable) — EXECUTING
Plan: 1 of 3
**Milestone:** v1 MVP
**Current Phase:** 4
**Current Plan:** Not started
**Status:** Ready to plan

```
Progress: [█░░░░░░] 1/7 phases complete
```

## Phase Status

| Phase | Status | Completed |
|-------|--------|-----------|
| 1. Foundation + Thread Model Spike | ✅ Complete (verified 4/4) | 2026-07-15 |
| 2. Activation | Not started | - |
| 3. Tray + Runtime + First Distributable | Not started | - |
| 4. Pusher Event Stream | Not started | - |
| 5. Job Pipeline | Not started | - |
| 6. Resilience | Not started | - |
| 7. Auto-Update + Distribution Polish | Not started | - |

## Performance Metrics

- Plans executed: 3
- Plans requiring repair: 0
- Repair success rate: —
- Phases completed: 1/7

## Accumulated Context

### Key Decisions Locked

| Decision | Rationale |
|----------|-----------|
| Rust nativo (não Tauri) | Agente headless always-on; UI mínima não justifica webview; menor footprint |
| Agente é spooler burro | Noren renderiza ESC/POS server-side; sem drift de layout/QR em Rust |
| Pusher (evento) + HTTP (payload) + ack | Evita limite ~10KB do Pusher; dá ack de entrega e fila server-side para offline |
| egui raw sobre `winit 0.30` (NOT eframe, NOT tao) | eframe + event loop = conflito Win32 (C2). **Revisado 2026-07-15 pela pesquisa da Fase 1:** `winit 0.30` substitui `tao` — tao 0.35 usa a API antiga de closure, incompatível com `egui-winit 0.35` (exige `winit ^0.30.13` ApplicationHandler). `tray-icon 0.24` suporta winit direto; tao é fork do winit. Ver `01-RESEARCH.md`. |
| **Core cross-platform (build/test no Linux) — produto v1 ainda Windows-only** | **Adicionado 2026-07-15 (dono):** o core portável (SQLite+migrations, app-dir via `dirs`, ConfigStore, trait `CredentialStore`, tipos de erro, e a janela winit/egui/wgpu) compila e testa no Linux E Windows. APIs só-Windows (`windows`, `windows-dpapi`, `tray-icon`, `printers`, `auto-launch`, `velopack`, toast) ficam em `[target.'cfg(windows)'.dependencies]` + `#[cfg(windows)]`. DPAPI real só no Windows; impl Linux `DevFileCredentialStore` é dev-only (NÃO seguro). CI matrix ubuntu+windows. Ver CONTEXT D-01..D-04 (rev) + D-20..D-24. Fase 1 re-planejada. |
| Velopack para auto-update | Bootstrapper evita FILE_SHARING_VIOLATION no EXE em execução; fallback RunOnce documentado |
| WritePrinter RAW (não escpos/CreateFile) | TM-T20X usa usbprint.sys; troca de driver WinUSB excluída do v1 |
| Serial validado pelo Noren | Reusa auth/tenant existente; sem infra de licenciamento separada |

### Critical Pitfalls (from research)

- C1: RAW datatype omitido no WritePrinter → ESC/POS vira lixo silenciosamente; validar no test-print
- C2: eframe + tao event-loop conflict → Phase 1 spike é gate para todo código de GUI
- C3: dedup in-memory perdido no crash → SQLite `printed_jobs` com `INSERT OR IGNORE` é o único fence correto
- C4: ack antes do print confirmado → job silenciosamente perdido; ordem: done no SQLite → AckSender → POST ack
- C5: Pusher zombie connection >5 min → ping/pong 30s obrigatório desde o primeiro dia

### Open Todos

- [ ] Phase 1 spike: confirm `tao` + raw `egui` approach (or document subprocess fallback)
- [ ] Coordinate Noren backend `/api/agent/` contract implementation (parallel workstream)
- [ ] Procure Authenticode OV certificate — CI signing step ready (CODESIGN_PFX_BASE64 + CODESIGN_PFX_PASSWORD secrets needed in GitHub repo). SC-4 (signed installer) gates on this.
- [ ] Plan SmartScreen reputation warm-up: OV-signed but new binary shows initial warnings; clears after ~hundreds of clean installs (D-14; see CLAUDE.md signing notes). No code task — timeline expectation for owner.

### Blockers

- Noren `POST /api/agent/activate` blocks Phase 2 completion
- Noren Pusher auth + event emission blocks Phase 4 completion
- Noren ESC/POS server-side rendering (longest-lead) blocks Phase 5 completion
- Noren `GET /api/agent/jobs/pending` blocks Phase 6 completion
- Noren `GET /api/agent/version` + update hosting blocks Phase 7 completion

## Session Continuity

**Last session:** 2026-07-16T15:00:15.762Z
**Next action:** `/gsd:plan-phase 2` — Plan Phase 2 (Activation). NOTE: Phase 2 completion is
blocked on Noren `POST /api/agent/activate` + `agent_serials` table; planning can start now.

---

*State initialized: 2026-07-15*
*Last updated: 2026-07-15 after Phase 01 completion + verification (4/4 PASS)*
