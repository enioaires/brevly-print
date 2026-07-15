---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
current_phase: 1 — Foundation + Thread Model Spike
current_plan: (none yet — planning not started)
status: Not started
last_updated: "2026-07-15T20:51:02.774Z"
progress:
  total_phases: 7
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
  percent: 0
---

# State: Brevly Print

## Project Reference

**Core Value:** Quando um evento de impressão chega do Noren, a comanda/cupom correto sai na impressora
térmica em menos de 1 segundo, de forma confiável e sem intervenção humana — nenhuma
comanda perdida, mesmo com impressora ou internet fora do ar.

**Stack:** Rust nativo (Windows-only) — tao + tray-icon + egui (raw) + tokio + reqwest (rustls) + rusqlite (bundled) + windows crate (WritePrinter RAW) + serialport + auto-launch + velopack + tauri-winrt-notification + windows-dpapi

**Cross-repo dependency:** Noren backend (`~/repos/brevly/noren`) must implement `/api/agent/*` contract before phases 2, 4, 5, 6, 7 can be completed. Server-side ESC/POS rendering is the longest-lead Noren item — gates Phase 5.

## Current Position

**Milestone:** v1 MVP
**Current Phase:** 1 — Foundation + Thread Model Spike
**Current Plan:** (none yet — planning not started)
**Status:** Not started

```
Progress: [░░░░░░░] 0/7 phases complete
```

## Phase Status

| Phase | Status | Completed |
|-------|--------|-----------|
| 1. Foundation + Thread Model Spike | Not started | - |
| 2. Activation | Not started | - |
| 3. Tray + Runtime + First Distributable | Not started | - |
| 4. Pusher Event Stream | Not started | - |
| 5. Job Pipeline | Not started | - |
| 6. Resilience | Not started | - |
| 7. Auto-Update + Distribution Polish | Not started | - |

## Performance Metrics

- Plans executed: 0
- Plans requiring repair: 0
- Repair success rate: —
- Phases completed: 0/7

## Accumulated Context

### Key Decisions Locked

| Decision | Rationale |
|----------|-----------|
| Rust nativo (não Tauri) | Agente headless always-on; UI mínima não justifica webview; menor footprint |
| Agente é spooler burro | Noren renderiza ESC/POS server-side; sem drift de layout/QR em Rust |
| Pusher (evento) + HTTP (payload) + ack | Evita limite ~10KB do Pusher; dá ack de entrega e fila server-side para offline |
| egui raw sobre `winit 0.30` (NOT eframe, NOT tao) | eframe + event loop = conflito Win32 (C2). **Revisado 2026-07-15 pela pesquisa da Fase 1:** `winit 0.30` substitui `tao` — tao 0.35 usa a API antiga de closure, incompatível com `egui-winit 0.35` (exige `winit ^0.30.13` ApplicationHandler). `tray-icon 0.24` suporta winit direto; tao é fork do winit. Ver `01-RESEARCH.md`. |
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
- [ ] Procure Authenticode OV certificate (needed for Phase 3 distributable)
- [ ] Plan SmartScreen reputation period (2-6 weeks after first signed release)

### Blockers

- Noren `POST /api/agent/activate` blocks Phase 2 completion
- Noren Pusher auth + event emission blocks Phase 4 completion
- Noren ESC/POS server-side rendering (longest-lead) blocks Phase 5 completion
- Noren `GET /api/agent/jobs/pending` blocks Phase 6 completion
- Noren `GET /api/agent/version` + update hosting blocks Phase 7 completion

## Session Continuity

**Last session:** 2026-07-15T20:51:02.767Z
**Next action:** `/gsd:plan-phase 1` — Plan the Foundation + Thread Model Spike

---

*State initialized: 2026-07-15*
*Last updated: 2026-07-15 after roadmap creation*
