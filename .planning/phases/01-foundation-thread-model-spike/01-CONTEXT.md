# Phase 1: Foundation + Thread Model Spike - Context

**Gathered:** 2026-07-15
**Status:** Ready for planning
**Note:** Owner delegated all decisions ("decide tudo da phase 1") — every gray area below is locked. Downstream agents should treat these as decided, not open.
**MAJOR REVISION 2026-07-15:** Owner added a requirement — **the portable core must build and `cargo test` on Linux too** (dev/test parity), while the **product v1 stays Windows-only**. Windows-only APIs are `#[cfg(windows)]`-gated with cross-platform abstractions + Linux dev/test impls. This revokes the "Windows-only build" stance of the original D-01..D-04 and adds D-20..D-24. See the **Cross-Platform Architecture** section.

<domain>
## Phase Boundary

Prove the highest-risk unknown in the project — the `winit 0.30` event loop + raw `egui`
rendering integration — and stand up all persistence infrastructure on a validated,
**cross-platform-buildable** base, so every later phase builds on proven ground. The
portable core builds and tests on **both Linux (primary dev box) and Windows**; the
product still ships Windows-only for v1.

**Delivers (the walking skeleton — thinnest end-to-end slice):**
- Cargo project compiles on **both** `x86_64-unknown-linux-gnu` (portable core + Linux dev
  impls) and `x86_64-pc-windows-msvc` (full v1 dep set incl. Windows-only crates)
- A `winit 0.30` `ApplicationHandler` event loop drives a minimal raw-`egui` window (one
  interactive frame: text field + button) via `egui-winit` + `egui-wgpu` — runs on Linux
  (Vulkan/GL) and Windows (DX12), no separate Win32 loop — OR the subprocess fallback is
  proven and documented as the Phase 2 approach
- SQLite `state.db` initializes with tables `config`, `printed_jobs`, `retry_queue`
  (versioned migration v1) at the per-platform app dir: `%APPDATA%\BrevlyPrint\` on
  Windows, `$XDG_DATA_HOME`/`~/.local/share/BrevlyPrint/` on Linux (via `dirs`)
- Credential store round-trips (encrypt → write → read → decrypt) through a
  `CredentialStore` trait; a missing/corrupted file returns a typed `CredentialError`
  instead of panicking. The **DPAPI** impl (Windows) and a **Linux dev impl** both satisfy
  the trait; DPAPI's real round-trip is proven on Windows, the trait/error contract is
  tested on both
- The app dir created idempotently on first run (both platforms)

**Explicitly NOT in this phase:** tray icon (Phase 3), Pusher/WebSocket (Phase 4),
any printing/WritePrinter/serial (Phase 5), real activation logic — serial validation,
printer dropdown, test-print (Phase 2), signing/packaging (Phase 3). Also NOT in scope:
**Linux as a shipping product** (v1 stays Windows-only — Linux is dev/test parity only).
Pure technical spike + infra: no v1 REQUIREMENTS IDs map here; it unblocks all of them.

</domain>

<decisions>
## Implementation Decisions

### Cross-Platform Build & Test Environment  *(REVISED 2026-07-15 — was "Windows-only")*
- **D-01 (REVISED):** The **portable core builds and `cargo test`s on Linux** (this dev
  box, native `x86_64-unknown-linux-gnu`) **and** on Windows. The Linux loop is now the
  **primary day-to-day dev/test loop**; the Windows box + CI validate the Windows-only
  paths (real DPAPI round-trip) and the visual window. No cross-compilation toolchain is
  required — each platform builds natively for its own host target; Windows-only crates are
  `cfg`-gated so Linux never links them (see D-20).
- **D-02 (REVISED):** Two host targets, both must compile: `x86_64-unknown-linux-gnu`
  (Linux dev/test) and `x86_64-pc-windows-msvc` (Windows product/CI). No cross-target
  builds — build on the platform you're on.
- **D-03 (REVISED):** **GitHub Actions CI matrix** from Phase 1: an `ubuntu-latest` job
  (`cargo build` + `cargo test` on the portable core — the fast gate) and a
  `windows-latest` job (`cargo build --release` + full `cargo test` incl. DPAPI, `WGPU_BACKEND=dx12`).
  No signing/packaging yet — Phase 3. Linux CI needs system libs for wgpu/winit
  (Vulkan loader + X11/Wayland dev headers) — install step included.
- **D-04 (REVISED):** Proof is split by platform: the **portable core** (SQLite schema,
  migrations, app-dir init, config store, credential trait + error paths, and — because
  winit/egui/wgpu are cross-platform — even the spike **window**) is proven on **Linux**
  via `cargo test` + `cargo run`. The **real DPAPI round-trip** and the **Windows visual
  window** are proven on the owner's Windows box + Windows CI. SC-1 can now be demonstrated
  on Linux first, then confirmed on Windows.

### Spike Disposition & Crate Layout
- **D-05:** The `winit`+`egui` spike **becomes the real `src/` foundation** (walking
  skeleton), not throwaway. `src/main.rs` owns the `winit` event loop and the minimal
  `egui` window; Phase 2 grows that window into the activation screen. One coherent
  codebase — the event-loop wiring is written once.
- **D-06:** **`lib` + `bin` split.** `src/lib.rs` exposes `ConfigStore`, the
  `CredentialStore` trait, and the app-dir init as a library; `src/main.rs` is the thin
  binary that wires the event loop. This lets `cargo test` exercise the SQLite schema and
  the credential contract as real integration tests (which both CI jobs run).
- **D-07 (REVISED):** Walking-skeleton scope = the thinnest working slice, **provable on
  Linux**: compiles (Linux + Windows) → `winit` window opens → one real `egui` interaction
  (text input + button reacting) → one real SQLite write+read (init schema, write and read
  back a `config` row) → one real credential encrypt+decrypt round-trip through the
  `CredentialStore` trait → app dir created. On Linux the credential round-trip uses the
  Linux dev impl; on Windows it uses DPAPI. Nothing more. (Planner: emit `SKELETON.md` per
  Walking-Skeleton mode.)

### egui Integration Risk & Fallback  *(Claude's discretion — owner said "você decide")*
- **D-08 (REVISED 2026-07-15 after Phase 1 research — supersedes original `tao` choice):**
  Primary approach: **raw `egui` rendered on the `winit 0.30` `ApplicationHandler` event
  loop** via `egui-winit` + `egui-wgpu` (wgpu DX12/WARP backend — reliable on Windows and
  headless CI). **`winit 0.30` replaces `tao`.** Reason: `tao 0.35` still uses the old
  closure-based `EventLoop::run()` API, which is incompatible with `egui-winit 0.35` (it
  requires `winit ^0.30.13`'s new `ApplicationHandler` trait). `tray-icon 0.24` supports
  `winit 0.30` directly (official `winit.rs` example) with no runtime coupling to tao; tao
  is itself a winit fork, so this moves to the upstream, better-supported path.
  `eframe::run_native()` remains **prohibited** (pitfall C2). See
  `01-RESEARCH.md` §Standard Stack and Patterns 1–2 for the verified crate combo + code.
- **D-09:** **Timebox ~1–2 focused days** on the embedded approach. Bar to clear: a
  single interactive `egui` frame (text field + button reacting to input) rendering
  inside the `winit` loop — provable on **Linux first** (Vulkan/GL), then confirmed on Windows.
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
- **D-13 (REVISED):** `state.db` lives in the per-platform app dir (see D-17):
  `%APPDATA%\BrevlyPrint\state.db` on Windows, `~/.local/share/BrevlyPrint/state.db` on
  Linux. `rusqlite` with `features = ["bundled"]` — zero external DLL, and bundled SQLite
  compiles identically on Linux. `rusqlite`/`rusqlite_migration` are cross-platform (portable core).
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

### Credential Store (trait + DPAPI, REVISED)
- **D-15 (REVISED):** Credentials go through a **`CredentialStore` trait**
  (`save(&[u8])` / `load() -> Result<Vec<u8>, CredentialError>`) with two impls behind
  `cfg`:
  - **Windows (`#[cfg(windows)]`) — `DpapiCredentialStore`:** `credential.bin` in the app
    dir, encrypted via `windows-dpapi` **`Scope::User`** (`CryptProtectData`). The real
    security path; proven on Windows.
  - **Non-Windows (`#[cfg(not(windows))]`) — `DevFileCredentialStore`:** a plain (or
    lightly-obfuscated) file in the Linux app dir, **explicitly marked dev/test-only, NOT
    secure**, so the core builds/tests on Linux. It exists to exercise the trait + error
    contract, never to ship as a product credential store.
  Both hold the `agentToken` (issued at activation, Phase 2). Phase 1 proves the
  encrypt/save→load/decrypt round-trip through whichever impl the platform selects.
- **D-16:** A missing or undecryptable credential returns a typed `CredentialError`
  (`NotFound` vs `Corrupt`) — **never panics**, on **both** impls. This is the exact hook
  Phase 2 uses to re-enter the activation flow after DPAPI key loss (pitfall M7). The
  error contract is unit-tested on Linux; the real DPAPI-corrupt case is tested on Windows.

### Directory Init & Error Handling
- **D-17 (REVISED):** The app dir is resolved via `dirs::data_dir()` (→ `%APPDATA%` on
  Windows, `$XDG_DATA_HOME`/`~/.local/share` on Linux) + `BrevlyPrint/`, created via
  `std::fs::create_dir_all` at startup (idempotent) **before** opening `state.db` or the
  credential file (pitfall m2). `dirs` is cross-platform; app-dir tests run on both.
- **D-18:** Library error types via **`thiserror`** (typed, matchable — e.g.
  `CredentialError`, `StoreError`); the binary/glue layer may use `anyhow` for
  top-level context.

### Cargo.toml Dependency Set  *(versions per RESEARCH.md; platform split added)*
- **D-19 (REVISED — versions + platform split):** Use the authoritative version table in
  `01-RESEARCH.md` §Standard Stack (all crates verified crates.io 2026-07-15), NOT the
  stale originals. **Dependencies are split by target so Linux never links Windows-only
  crates:**
  - **`[dependencies]` (portable — build on Linux + Windows):** `winit`, `egui`,
    `egui-winit`, `egui-wgpu`, `wgpu`, `rusqlite` (`bundled`), `rusqlite_migration`,
    `dirs`, `serde`, `serde_json`, `thiserror`, `anyhow`, `tokio`, `reqwest`
    (`rustls-tls`,`json`), `tokio-tungstenite`, `hmac`, `sha2`, `serialport`.
  - **`[target.'cfg(windows)'.dependencies]` (Windows-only):** `windows` (0.62),
    `windows-dpapi` (0.2), `tray-icon` (0.24), `printers`, `auto-launch`, `velopack`,
    `tauri-winrt-notification`. (Full v1 set still present so the Windows CI job proves
    SC-4; Linux compiles the portable subset.)
  - Version bumps from the original stack: `winit 0.30.13` replaces `tao`; `egui` 0.31→0.35;
    `rusqlite` 0.32→0.40; `tray-icon` 0.21→0.24; etc. — see RESEARCH.md §Standard Stack.
  - The planner should verify each "portable" crate genuinely builds on Linux; if any
    surprises appear (e.g. `serialport` needing `libudev` on Linux — add the system dep or
    gate it), move it to the Windows-only block and note it. `tray-icon` is Windows-only
    here anyway (Phase 3).

### Cross-Platform Architecture  *(NEW 2026-07-15 — the "works on Linux too" requirement)*
- **D-20:** **`#[cfg(windows)]` / `#[cfg(not(windows))]` gating** isolates every OS-specific
  surface. Windows-only modules (DPAPI credential impl; later: printing, tray, autostart,
  toast, velopack) are gated; the portable core (SQLite, migrations, app-dir, config,
  error types, the `CredentialStore` trait, the winit/egui window) compiles everywhere.
- **D-21:** **Trait-based platform abstraction** for anything with a real Linux
  counterpart-or-stub. Phase 1: `CredentialStore` trait (D-15). This sets the pattern
  later phases follow (e.g. a `Printer` trait in Phase 5 — out of scope now, just the
  precedent). The binary selects the impl via `cfg` at construction.
- **D-22:** **Tests run on Linux for everything portable** and are `#[cfg(windows)]`-gated
  only where they need a real Windows API (the true DPAPI corrupt-blob case). No test
  requires Windows to exercise the SQLite schema, migrations, app-dir, config, or the
  credential trait/error contract.
- **D-23:** **CI matrix** (D-03): `ubuntu-latest` (portable build+test, the fast default
  gate) + `windows-latest` (full build+test incl. DPAPI). Linux job installs wgpu/winit
  system deps (Vulkan loader, X11/Wayland headers). Product artifacts remain Windows-only.
- **D-24:** **Product v1 is still Windows-only.** Linux is dev/test parity, NOT a shipping
  target. The Linux credential impl (`DevFileCredentialStore`) is dev-only and must never
  be presented as a secure production store. Shipping on Linux is a deferred idea, not v1.

### Claude's Discretion
- Exact `egui`/`egui-wgpu`/`wgpu` crate glue and window-creation boilerplate (cross-platform).
- Exact SQLite column types/indexes within the D-14 shape.
- Test harness layout within the D-06 split; which tests are `#[cfg(windows)]`-gated.
- Module layout for the `cfg`-gated credential impls (e.g. `credential_store/{mod,dpapi,devfile}.rs` vs inline `cfg`).
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
