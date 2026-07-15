# Phase 1: Foundation + Thread Model Spike - Context

**Gathered:** 2026-07-15
**Status:** Ready for planning
**Note:** Owner delegated all decisions ("decide tudo da phase 1") — every gray area below is locked. Downstream agents should treat these as decided, not open.

<domain>
## Phase Boundary

Prove the highest-risk unknown in the project — the `tao` event loop + raw `egui`
rendering integration on Windows — and stand up all persistence infrastructure on a
validated Windows base, so every later phase builds on proven ground.

**Delivers (the walking skeleton — thinnest end-to-end slice):**
- Cargo project compiles a Windows-target binary with the full v1 dependency set
- A `tao::EventLoop` drives a minimal raw-`egui` window (one interactive frame:
  text field + button) on Windows, with **no** separate Win32 message loop — OR the
  subprocess fallback is proven and documented as the Phase 2 approach
- SQLite `state.db` initializes at `%APPDATA%\BrevlyPrint\state.db` with tables
  `config`, `printed_jobs`, `retry_queue` (versioned migration v1)
- DPAPI `credential.bin` round-trips (encrypt → write → read → decrypt); a
  missing/corrupted file returns a typed `CredentialError` instead of panicking
- `%APPDATA%\BrevlyPrint\` directory created idempotently on first run

**Explicitly NOT in this phase:** tray icon (Phase 3), Pusher/WebSocket (Phase 4),
any printing/WritePrinter/serial (Phase 5), real activation logic — serial validation,
printer dropdown, test-print (Phase 2), signing/packaging (Phase 3). Pure technical
spike + infra: no v1 REQUIREMENTS IDs map here; it unblocks all of them.

</domain>

<decisions>
## Implementation Decisions

### Windows Build & Test Environment
- **D-01:** Compile and run **natively on the Windows machine/VM** (owner has one).
  `git pull` + `cargo run`/`cargo test` happen on Windows; the Linux box is for
  planning/git/GSD only. No cross-compilation toolchain in v1 — avoids cross-linking
  the `windows` crate + `egui`/`wgpu`, which would add friction to exactly the spike
  we're de-risking.
- **D-02:** Target triple `x86_64-pc-windows-msvc`.
- **D-03:** Set up a **GitHub Actions Windows runner** from Phase 1 as a **build-only
  gate**: `cargo build --release` + `cargo test` on every push. This automates
  success-criterion 4 ("compiles the full dependency set for Windows") and catches
  dependency breakage early. No signing/packaging in CI yet — that arrives with the
  installer pipeline in Phase 3.
- **D-04:** The spike is **proven interactively on the Windows box** (window renders,
  interaction works, DPAPI round-trips, SQLite + `%APPDATA%` init) — CI proves
  compilation, the owner's Windows machine proves runtime behavior.

### Spike Disposition & Crate Layout
- **D-05:** The `tao`+`egui` spike **becomes the real `src/` foundation** (walking
  skeleton), not throwaway. `src/main.rs` owns the `tao::EventLoop` and the minimal
  `egui` window; Phase 2 grows that window into the activation screen. One coherent
  codebase — the event-loop wiring is written once.
- **D-06:** **`lib` + `bin` split.** `src/lib.rs` exposes `ConfigStore`,
  `CredentialStore`, and the `%APPDATA%` init as a library; `src/main.rs` is the thin
  binary that wires the event loop. This lets `cargo test` exercise the SQLite schema
  and DPAPI round-trip as real integration tests (which the CI gate runs).
- **D-07:** Walking-skeleton scope = the thinnest working slice: compiles on Windows →
  `tao` window opens → one real `egui` interaction (text input + button reacting) →
  one real SQLite write+read (init schema, write and read back a `config` row) → one
  real DPAPI encrypt+decrypt round-trip → `%APPDATA%\BrevlyPrint\` created. Nothing
  more. (Planner: emit `SKELETON.md` per Walking-Skeleton mode.)

### egui Integration Risk & Fallback  *(Claude's discretion — owner said "você decide")*
- **D-08:** Primary approach: **raw `egui` rendered inside the `tao::EventLoop`** via
  `egui-wgpu` (DirectX backend — reliable across Windows GPU/driver setups).
  `eframe::run_native()` is **prohibited** (dual Win32 event-loop conflict — pitfall C2).
- **D-09:** **Timebox ~1–2 focused days** on the embedded approach. Bar to clear: a
  single interactive `egui` frame (text field + button reacting to input) rendering
  inside the `tao` loop on the Windows box.
- **D-10:** If the timebox is hit or embedding fails on a hard technical wall,
  **auto-switch to the subprocess fallback** (separate short-lived setup-window
  process; result returned via temp file or named pipe), **document the evidence** in
  the spike notes, and **flag it for owner review** — no blocking handoff (fits the
  "develop via GSD, owner just reviews" model). This pivot reshapes Phase 2, so the
  chosen path MUST be recorded as the Phase 2 activation-window approach.
- **D-11:** Renderer fallback: if `egui-wgpu` bloats the binary unacceptably or proves
  flaky on the target, evaluate `egui-glow` (OpenGL). The spike validates the choice.

### SQLite Persistence & Schema Versioning  *(Claude's discretion — owner said "você decide")*
- **D-12:** **Versioned migrations from day one** via `rusqlite_migration`
  (`user_version` pragma). Phase 1 registers **migration v1 = the three tables**;
  future phases append vN. Non-negotiable because auto-update (DIST-02) ships schema
  changes to already-installed field agents — ordered, tracked migrations are the only
  safe upgrade path. Plain `CREATE TABLE IF NOT EXISTS` is rejected (no ordering, no
  post-ship column-change story).
- **D-13:** `state.db` at `%APPDATA%\BrevlyPrint\state.db`; `rusqlite` with
  `features = ["bundled"]` — zero external DLL.
- **D-14:** Schema v1 (planner finalizes exact column types from research/ARCHITECTURE.md,
  following this shape). SQLite is a **dedup tracker + retry coordinator, NOT the
  authoritative job queue** (Noren owns that). Phase 1 only creates the schema and
  proves one write/read; later-phase columns are provisioned now:
  - `config` — key/value: `key TEXT PRIMARY KEY, value TEXT NOT NULL` (rows for
    `printer_name`, `printer_type`, `tenant_id`, `enabled_types`). Key/value keeps
    future config additions migration-free.
  - `printed_jobs` — dedup fence: `job_id TEXT PRIMARY KEY, status TEXT NOT NULL,
    created_at TEXT, printed_at TEXT`; dedup via `INSERT OR IGNORE`; status ∈
    {`printing`, `done`} (pitfall C3).
  - `retry_queue` — `job_id TEXT PRIMARY KEY, job_type TEXT, escpos_bytes BLOB,
    attempt_count INTEGER NOT NULL DEFAULT 0, next_retry_at TEXT, last_error TEXT,
    created_at TEXT`.

### Credential Store (DPAPI)
- **D-15:** `credential.bin` at `%APPDATA%\BrevlyPrint\credential.bin` via
  `windows-dpapi`, **`Scope::User`** (`CryptProtectData`). Holds the `agentToken`
  (issued at activation, Phase 2). Phase 1 proves the encrypt→write→read→decrypt
  round-trip with a dummy value.
- **D-16:** A missing or undecryptable `credential.bin` returns a typed
  `CredentialError` — **never panics**. This is the exact hook Phase 2 uses to
  re-enter the activation flow after DPAPI key loss (pitfall M7: Windows reinstall →
  new SID → unreadable credential).

### Directory Init & Error Handling
- **D-17:** `%APPDATA%\BrevlyPrint\` created via `std::fs::create_dir_all` at startup
  (idempotent) **before** opening `state.db` or `credential.bin` (pitfall m2).
- **D-18:** Library error types via **`thiserror`** (typed, matchable — e.g.
  `CredentialError`, `StoreError`); the binary/glue layer may use `anyhow` for
  top-level context.

### Cargo.toml Dependency Set
- **D-19:** The **full v1 dependency set** must be present and compiling on the Windows
  target from Phase 1 (success-criterion 4), even for crates not exercised until later
  phases: `tao` 0.35, `tray-icon` 0.21, `egui` 0.31 + `egui-wgpu`, `tokio` 1.x,
  `reqwest` 0.13 (`default-features = false`, `features = ["rustls-tls", "json"]`),
  `rusqlite` 0.32 (`features = ["bundled"]`) + `rusqlite_migration`, `windows` 0.62
  (`Win32_Graphics_Printing` + printing/DPAPI feature flags), `windows-dpapi`,
  `serialport` 4.7, `printers` 0.2, `auto-launch` 0.5, `velopack`,
  `tauri-winrt-notification` 0.5, `tokio-tungstenite` 0.26, `hmac`, `sha2`,
  `thiserror`, `anyhow`. Exact patch versions pinned by the planner against crates.io.

### Claude's Discretion
- Exact `egui`/`egui-wgpu`/`wgpu`/`glow` crate glue and window-creation boilerplate.
- Exact SQLite column types/indexes within the D-14 shape.
- Test harness layout (where DPAPI/SQLite integration tests live) within the D-06 split.
- Subprocess-fallback IPC mechanism (temp file vs named pipe) — decided only if D-10 triggers.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project research (primary source of truth for the stack)
- `.planning/research/SUMMARY.md` — reconciled stack decisions (egui-raw-not-eframe,
  Velopack, WritePrinter RAW, hand-rolled Pusher), two-thread-domain architecture, and
  the Phase 1 scope/rationale (§"Phase 1: Foundation + Thread Model Spike")
- `.planning/research/STACK.md` — crate versions and exact feature flags
- `.planning/research/ARCHITECTURE.md` — component graph and the SQLite table roles
  (ConfigStore / CredentialStore definitions)
- `.planning/research/PITFALLS.md` — C2 (event-loop conflict), M7 (DPAPI key loss),
  m2 (directory init), C3 (dedup fence)
- `.planning/research/FEATURES.md` — feature tiers (what's v1 vs deferred)

### Project planning docs
- `.planning/ROADMAP.md` §"Phase 1" — the four success criteria this phase is graded on
- `.planning/PROJECT.md` §"Key Decisions" — locked project-level decisions
- `.planning/STATE.md` §"Accumulated Context" — locked decisions + pitfalls C1–C5
- `CLAUDE.md` — project constraints and the recommended-stack tables

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- **Greenfield repo** — no agent code exists yet. This phase creates the initial
  Cargo project (`Cargo.toml`, `src/main.rs`, `src/lib.rs`).

### Established Patterns
- None in-repo yet. The research docs (above) are the pattern source. Phase 1
  establishes the `lib`+`bin` layout and the migration-based schema pattern that every
  later phase extends.

### Integration Points
- **Noren backend** (`~/repos/brevly/noren`, separate repo) is the eventual integration
  surface but is **not touched in Phase 1**. The client-side ESC/POS builders in
  `src/lib/utils/ticket.ts` are the reference for Noren's *server-side* rendering
  migration (a Phase 5 prerequisite), not for this phase.

</code_context>

<specifics>
## Specific Ideas

- The window shown in Phase 1 is a **spike stub**, not the activation UI — it exists
  only to prove the render/interaction thread model. Its one interaction (text field +
  button) is deliberately minimal.
- CI is intentionally **build-only** in Phase 1 (`cargo build --release` + `cargo test`);
  signing/packaging stays out until Phase 3 where it's actually needed.

</specifics>

<deferred>
## Deferred Ideas

- **Tray icon rendering** (green/yellow/red state machine) — Phase 3.
- **Signing, `vpk` packaging, VirusTotal/CI signing steps** — Phase 3.
- **Real activation window** (serial input, combined printer/COM dropdown, test-print) —
  Phase 2, on whichever GUI path D-08/D-10 validates.
- **Subprocess-fallback IPC design** — only if D-10 triggers; would become the Phase 2
  activation approach.

None of these were scope creep — they surfaced naturally while bounding the spike and
are already mapped to their owning phases in ROADMAP.md.

</deferred>

---

*Phase: 01-foundation-thread-model-spike*
*Context gathered: 2026-07-15*
