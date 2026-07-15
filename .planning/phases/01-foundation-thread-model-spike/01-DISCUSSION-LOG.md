# Phase 1: Foundation + Thread Model Spike - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-07-15
**Phase:** 01-foundation-thread-model-spike
**Areas discussed:** Windows build & test setup, Spike disposition, egui fallback decision rule, SQLite schema versioning

---

## Windows Build & Test Setup

### Windows environment available

| Option | Description | Selected |
|--------|-------------|----------|
| A real Windows machine/VM I can use | Full interactive verification of window + DPAPI + %APPDATA% | ✓ |
| Only CI (GitHub Actions Windows runner) | Compile + headless checks; no visual window inspection | |
| Nothing yet — need to set one up | Blocker to resolve before spike can be proven | |

**User's choice:** A real Windows machine/VM.

### Build/iteration loop

| Option | Description | Selected |
|--------|-------------|----------|
| Build & run natively on Windows | cargo compile+run on Windows; Linux for planning/git only | ✓ |
| Cross-compile from Linux, run on Windows | cargo-xwin/msvc target on Linux, copy .exe | |
| You decide | Planner picks least-risky | |

**User's choice:** Build & run natively on Windows.

### CI timing

| Option | Description | Selected |
|--------|-------------|----------|
| Set up CI now (build-only) | Windows CI runs cargo build --release + cargo test from Phase 1 | ✓ |
| Defer CI to Phase 3 | Manual verification now; CI with signing pipeline later | |

**User's choice:** Set up CI now (build-only).

**Notes:** Rationale — success-criterion 4 ("compiles the Windows target with full
dependency set") is best enforced by an automated gate; runtime GUI/DPAPI behavior is
verified interactively on the owner's Windows box.

---

## Spike Disposition

### Disposition of spike code

| Option | Description | Selected |
|--------|-------------|----------|
| Becomes the real src/ foundation | Spike IS the walking skeleton; Phase 2 grows it | ✓ |
| Throwaway proof, then rebuild clean | examples/ proof, discarded after | |

**User's choice:** Becomes the real src/ foundation.

### Crate layout

| Option | Description | Selected |
|--------|-------------|----------|
| lib + bin split | Testable ConfigStore/CredentialStore via cargo test | ✓ |
| Single binary crate | Simpler; harder to unit-test internals | |
| You decide | Planner chooses | |

**User's choice:** lib + bin split.

**Notes:** Walking-Skeleton mode is active for Phase 1 — the split supports the DPAPI/SQLite
acceptance tests the success criteria demand.

---

## egui Fallback Decision Rule

### Effort before switching to subprocess fallback

| Option | Description | Selected |
|--------|-------------|----------|
| Timebox it (~1–2 focused days) | Bounded attempt at embedded egui, then fall back | |
| No timebox — make embedded egui work | Fallback only on hard dead-end | |
| Skip the risk — go subprocess from the start | Two processes + IPC from day one | |

**User's choice:** "Você decide" (Claude's discretion).

### Who decides the switch

| Option | Description | Selected |
|--------|-------------|----------|
| Auto-switch + document it | Agent switches on rule, documents, flags for review | |
| Stop and ask me first | Halt and get owner approval before pivot | |

**User's choice:** "Você decide" (Claude's discretion).

**Notes:** Claude locked the recommended defaults — timebox ~1–2 days on embedded
egui-in-tao (bar: one interactive frame renders), then auto-switch to the subprocess
fallback with documented evidence + a review flag. See CONTEXT.md D-08–D-11.

---

## SQLite Schema Versioning

| Option | Description | Selected |
|--------|-------------|----------|
| Migration library w/ schema_version | rusqlite_migration + user_version from day one | |
| CREATE TABLE IF NOT EXISTS on startup | Idempotent creates, no version tracking | |
| You decide | Planner picks (likely versioned) | |

**User's choice:** "Você decide" (Claude's discretion).

**Notes:** Claude locked versioned migrations (rusqlite_migration) — auto-update
(DIST-02) ships schema to field agents, so ordered/tracked migrations are the only safe
path. See CONTEXT.md D-12.

---

## Claude's Discretion

Mid-discussion the owner delegated everything ("pode decidir tudo da phase 1, decide tudo").
Claude locked recommended defaults for:
- egui fallback timebox + auto-switch rule (D-08–D-11)
- SQLite versioned migrations (D-12)
- Exact egui/wgpu glue, SQLite column types within the D-14 shape, test-harness layout,
  and subprocess IPC mechanism (decided only if the fallback triggers)

## Deferred Ideas

- Tray icon rendering (green/yellow/red) — Phase 3
- Signing, vpk packaging, VirusTotal/CI signing — Phase 3
- Real activation window (serial input, printer/COM dropdown, test-print) — Phase 2
- Subprocess-fallback IPC design — only if the egui fallback triggers; becomes Phase 2 approach
