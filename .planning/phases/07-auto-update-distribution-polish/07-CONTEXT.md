# Phase 7: Auto-Update + Distribution Polish - Context

**Gathered:** 2026-07-16
**Status:** Ready for planning
**Note:** Owner delegated all four gray areas ("pode decidir tudo") — same posture as Phases 1
and 3. Every decision below is **locked** (Claude's discretion, exercised). Downstream agents
should treat these as decided, not open. Where a call is a genuine implementation detail (exact
Velopack Rust API, dependency versions), it is flagged for researcher/planner but still given a
recommended default so nothing stalls.

<domain>
## Phase Boundary

Make the agent keep **itself** current. Today the agent ships (Phase 3) and runs (Phases 4–6),
but never updates. This phase adds the **auto-update loop**: the running agent learns a newer
version exists, downloads it in the background **without interrupting printing**, verifies its
**SHA256 integrity before touching anything**, stages it, and — on the **next reboot/login** —
comes up running the new binary with **zero action from the restaurant owner**.

The apply mechanism already exists: `velopack::VelopackApp::build().run()` is the **first call in
`main()`** (Phase 3, OQ3/D-09). It applies any staged update on launch. This phase builds the
**check → download → verify → stage** half that feeds it, plus closes the release/publish loop so
producing an update is one CI run.

Covers requirements **DIST-02** (auto-update, applied on next restart, no owner action) and
**DIST-03** (SHA256 integrity verified before applying).

**Success Criteria this phase is graded on (from ROADMAP.md):**
1. New version available → downloads in background **without interrupting printing**; tray shows a
   brief "update ready" notification.
2. Downloaded binary's SHA256 is **verified against the value from `/api/agent/version` before the
   update is scheduled** — a mismatch **aborts** without touching the running agent.
3. On the next Windows reboot after a pending update, the new version is running — **no manual
   action** from the owner.

**Out of scope (belongs elsewhere / stays external):**
- **OV certificate procurement** (DIST-01 completion gate) — a real external blocker carried from
  Phase 3 D-12/D-14. This phase does **not** procure the cert and does **not** block on it. The
  `signtool` CI step stays gated (runs when the cert secret is present, skips cleanly otherwise).
- **Noren backend work** — hosting the update artifact on S3/Cloudflare and serving
  `GET /api/agent/version` is a Noren-side dependency (like the backend deps in Phases 2/4/5/6).
  Phase 7 builds and tests the **agent side** against that contract.
- **Branded tray artwork** — deferred from Phase 3 D-05; not pulled forward here.
- **Immediate/idle-time self-restart to apply sooner** — v1 applies strictly on next natural
  reboot/login (see D-05). Faster-apply is a deferred idea.
</domain>

<decisions>
## Implementation Decisions

### GA1 — Update source contract (who is authoritative, who applies)
- **D-01: Noren's `GET /api/agent/version` is the authoritative version signal; Velopack remains
  the apply mechanism.** The agent polls the endpoint (roadmap contract, LOCKED) and compares its
  `version` field to `env!("CARGO_PKG_VERSION")`. The `downloadUrl` points at the Velopack update
  package (`.nupkg`, produced by `vpk pack`) hosted on S3/Cloudflare. We do **not** require Noren
  to also stand up a full Velopack `releases.*.json` feed — the custom endpoint **is** the feed;
  the agent bridges to Velopack for the actual apply.
  - **Why Velopack still owns apply, not a hand-rolled swap:** safely replacing a **locked, running
    Windows `.exe`** on next launch is exactly the problem Velopack exists to solve. CLAUDE.md
    explicitly rejects `self_update` for this reason ("can't replace locked EXE on Windows
    cleanly; no installer story"). Going fully custom re-introduces the problem we already chose
    Velopack to avoid. So: **custom endpoint decides *whether/what*; Velopack executes the *how*.**
  - **Planner/researcher to resolve (technical, not owner-facing):** the exact Velopack **Rust**
    API for staging a locally-obtained package and applying it on next boot (candidates:
    `UpdateManager` pointed at the `downloadUrl` / a local package dir → `check_for_updates` →
    `download_updates` → `wait_exit_then_apply_updates`). If the Velopack Rust SDK cannot consume
    an arbitrary artifact and strictly needs its own feed layout, the fallback is to host the
    standard Velopack feed files at `downloadUrl`'s directory and point `UpdateManager` there —
    the endpoint still gates the check and supplies the authoritative `sha256` for D-02. Confirm
    against the Velopack Rust docs during research.

### GA2 — SHA256 verification ownership (DIST-03)
- **D-02: Explicit, manual SHA256 check is the authoritative DIST-03 gate — belt-and-suspenders on
  top of anything Velopack does internally.** After downloading the artifact, compute its SHA256
  and compare (constant-length hex, case-insensitive) against the `sha256` returned by
  `/api/agent/version`. This is what SC-2 demands literally.
  - **On mismatch:** abort. Do **not** stage, do **not** invoke Velopack apply, do **not** touch
    the running agent — it keeps running the current version. Log the mismatch (never log the
    token). No owner-facing error toast for a mismatch (it's an infra problem, not the owner's).
  - **Add `sha2 = "0.10"` (RustCrypto)** — pure Rust, Linux-testable. Planner to confirm it isn't
    already pulled transitively (Pusher HMAC path). Keep the check in a **pure function**
    `verify_sha256(bytes, expected_hex) -> Result<()>` so it unit-tests on Linux (match / mismatch
    / wrong-length / malformed hex) with no Windows dependency.

### GA3 — Update-check cadence & non-interference
- **D-03: Check on startup, then poll periodically (~6h), always off the print critical path.**
  - **Startup check:** a few seconds after boot, **after** the tray is up and the Pusher connect
    is underway — must **never** delay tray appearance or block printing (SC-1).
  - **Periodic re-check:** restaurant PCs may run for days/weeks. A long-running agent that only
    checked at boot would never *discover* an update until a reboot. Poll every ~6h (planner may
    tune) so an always-on agent stages the update ahead of the owner's next reboot.
  - **Failure handling:** a failed version-check (network down, endpoint 5xx, malformed JSON) is
    **silent** — log and retry on the next tick; no owner-facing error. Never let the update task
    interfere with the Pusher/print/retry tasks. Same tokio-task + `EventLoopProxy` pattern the
    other background tasks use.

### GA4 — "Update ready" notification UX (SC-1)
- **D-04: Quiet, two-tier signal; never nag, never force a restart.** When an update is
  downloaded, verified, and staged:
  - **Persistent:** update the tray's disabled status/menu line (and tooltip) to a plain PT-BR
    "update ready" state, e.g. **"Atualização pronta — será aplicada ao reiniciar"** (reuse
    `tray_runtime.rs`'s menu/status update path — same mechanism as the health status line).
  - **Brief notification (satisfies SC-1's "brief notification"):** **one** Windows toast via the
    Phase 6 `tauri-winrt-notification` infra, e.g. **"Brevly Print: atualização pronta. Será
    aplicada no próximo reinício."** Fire once at staging completion — do **not** re-toast on every
    poll.
  - **Do NOT hijack the tray icon color.** Green/yellow/red means *connection health* (Phase 3
    D-01/D-02, Phase 4). "Update ready" is orthogonal — it lives in the status-line text + the
    one-shot toast, not the icon color.

### GA5 — Apply timing (SC-3)
- **D-05: Apply strictly on next natural launch via the already-wired Velopack bootstrapper — no
  forced restart.** "Next reboot" in the roadmap = next login launch, since the agent autostarts
  (HKCU Run, Phase 2 D-13). Staging just leaves the update in Velopack's pending location; the
  `VelopackApp::build().run()` bootstrapper swaps it in on the next start. We do **not**
  auto-restart the agent to apply immediately — that would interrupt printing and break the
  "silent, no disruption, no owner action" promise (SC-1/SC-3). Consequence, accepted: an
  always-on machine won't run the new code until it reboots — this matches the roadmap's explicit
  "on next reboot" wording.

### GA6 — Scope of "Distribution Polish"
- **D-06: "Polish" = close the release/publish loop around the update artifact, NOT re-open
  signing procurement or UI.** Concretely, this phase's polish work is:
  - **Version-bump discipline:** `Cargo.toml` `version` is the single source of truth;
    `env!("CARGO_PKG_VERSION")` is the comparison base for D-01.
  - **Extend the Phase 3 CI** (`vpk pack` + conditional `signtool`) to also produce the **update
    package** and surface the three values Noren's `/api/agent/version` must serve — `version`,
    `downloadUrl`, `sha256` — so that once the OV cert lands, a signed + published update is one CI
    run. The actual S3/CF upload + Noren endpoint wiring is the **Noren-backend dependency**.
  - **Signing stays gated exactly as Phase 3 D-12** (cert = external blocker; step skips cleanly
    when the secret is absent). Phase 7 does **not** procure the cert and does **not** ship
    branded icons (deferred).

### Cross-platform / testability seams (established project pattern — Linux-provable core)
- **D-07: Update module is `#[cfg(windows)]`-gated for the Velopack apply + toast; the decision
  logic is pure and unit-tested on Linux.** Mirrors the whole-project pattern (Windows-only pieces
  gated, portable core builds+tests on Linux). Isolate:
  - `check_for_update(current_version, endpoint_response) -> UpdateDecision` — **pure**, Linux
    unit tests: newer / older / equal / malformed / missing fields → the right decision.
  - `verify_sha256(bytes, expected_hex) -> Result<()>` — **pure**, Linux unit tests (D-02).
  - Download + Velopack stage/apply + toast — Windows-gated behind a cfg/trait; Linux dev build
    compiles with a no-op/log stub so the surrounding logic stays testable.

### Claude's Discretion (delegated — planner/executor finalize)
- Exact Velopack Rust API for local-artifact staging vs. feed-directory fallback — D-01.
- Whether `GET /api/agent/version` needs `.bearer_auth(agent_token)` or is unauthenticated —
  reuse the `noren_client.rs` authenticated-GET pattern by default; confirm with backend (a public
  version manifest could be unauthenticated, but consistency argues for bearer).
- Poll interval tuning (default ~6h) and startup-check delay — D-03.
- Exact PT-BR copy for the status line + toast — D-04 (keep plain, non-technical).
- `sha2` version + whether it's already transitively present — D-02.
- CI job structure for producing/publishing the update package — D-06.
</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase requirements & grading
- `.planning/ROADMAP.md` → **Phase 7: Auto-Update + Distribution Polish** — Goal, the **3 Success
  Criteria** this phase is graded on, and the **Noren dependency contract**: `GET /api/agent/version`
  returns `{version, downloadUrl, sha256}`; update binary hosted at `downloadUrl` (S3/Cloudflare).
- `.planning/REQUIREMENTS.md` → **DIST-02** (auto-update on next restart, no owner action) and
  **DIST-03** (SHA256 integrity check before applying). DIST-01 (Authenticode signing) is Phase 3 +
  an external cert gate — not re-opened here.

### The apply mechanism this phase feeds (already wired)
- `src/main.rs` — `velopack::VelopackApp::build().run()` is the **first call in `main()`** (~line
  282; must stay first — OQ3). The single-instance mutex, tokio runtime, `EventLoopProxy`, and the
  background-task spawn block (Pusher / print worker / retry task) are all here — the update-check
  task slots into the **same spawn + `UserEvent` proxy pattern** (see `HealthChanged` at ~line 201,
  the retry proxy at ~line 450, the pusher proxy at ~line 479).
- `src/tray_runtime.rs` — the tray status/menu line + tooltip update path (`apply_health`,
  `build_tray_menu`, `TrayMenuItems`) reused for the D-04 "update ready" line; `show_about_dialog`
  already reads `env!("CARGO_PKG_VERSION")` (the D-01 comparison base).
- `src/noren_client.rs` — the HTTP pattern `GET /api/agent/version` follows: `noren_base_url()`
  (reads `NOREN_BASE_URL` compile-time env), a shared `reqwest::Client`, `.bearer_auth(agent_token)`
  for authenticated endpoints, `{base_url}/api/agent/...` URLs. New `check_version()` mirrors
  `fetch_pending_jobs()` / `fetch_job_bytes()`.

### Carried decisions, pitfalls & the signing gate
- `.planning/phases/03-tray-runtime-first-distributable/03-CONTEXT.md` → **D-09** (Velopack
  bootstrapper stays first call), **D-12** (`vpk pack` + conditional `signtool` CI pipeline; OV
  cert = external blocker — the pipeline this phase extends), **D-14** (SmartScreen reputation
  timeline — owner expectation, not a code task). Its `<deferred>` already routes DIST-02/03 here.
- `.planning/STATE.md` → "Accumulated Context": locked decisions ("Velopack for update"; cross-
  platform core / Windows-only product) and the open todos **"Procure Authenticode OV certificate"**
  and **"Plan SmartScreen reputation period"** (both external, both carried, neither blocks Phase 7
  code).

### Stack references (from CLAUDE.md)
- **CLAUDE.md → "Auto-Update"** — `velopack` crate (Rust SDK, "spike in Phase 1" note) + **`vpk`
  CLI** 1.2.x (builds installer *and* update package — same toolchain as Phase 3 packaging).
- **CLAUDE.md → "Windows Installer + Code Signing"** + Confidence Assessment ("Auto-update: MEDIUM
  — Velopack Rust SDK is newer") — basis for treating the exact Velopack Rust API as a research item.
- **CLAUDE.md → Alternatives Considered** — the `self_update` rejection ("can't replace locked EXE
  on Windows cleanly") is the rationale for D-01 keeping Velopack as the apply mechanism.
- `hmac`/`sha2` (RustCrypto) noted in CLAUDE.md for Pusher auth — same `sha2` family used for the
  D-02 integrity check.

### Cargo / gating facts (verified during scout)
- `Cargo.toml` — `velopack = "1"` (line 86) and `tauri-winrt-notification = "0.8"` (line 89) are
  both under `[target.'cfg(windows)'.dependencies]` (line 54). `sha2` is **not** yet present —
  add per D-02. Package `version = "0.1.0"` is the D-01 comparison base.
</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/main.rs` — the Velopack bootstrapper (already first call), the tokio runtime, and the
  background-task spawn block. The update-check task is a **new sibling** to the Pusher/print/retry
  tasks, using the same `event_loop.create_proxy()` → `UserEvent` forwarding pattern.
- `src/noren_client.rs` — `noren_base_url()`, shared `reqwest::Client`, `.bearer_auth()` GET
  pattern. `check_version()` is a near-clone of the existing job/pending fetchers.
- `src/tray_runtime.rs` — status-line / menu-item / tooltip update mechanism for the D-04 "update
  ready" signal; `env!("CARGO_PKG_VERSION")` already surfaced in `show_about_dialog`.
- Phase 6 `tauri-winrt-notification` toast infra — reused for the one-shot D-04 update toast.

### Established Patterns
- **All tray/GUI mutation on the event-loop thread; background tasks push via `EventLoopProxy` +
  `UserEvent`** (Pattern from Phase 1, enforced by pitfall C2). The update task follows it — add a
  `UserEvent` variant (e.g. `UpdateStaged`) rather than mutating the tray directly.
- **`#[cfg(windows)]` gating with a Linux-provable pure core** — D-07. Velopack apply + toast are
  Windows-only; version-compare + SHA256-verify are pure and Linux-tested.
- **Velopack bootstrapper stays the first call in `main()`** (OQ3) — do not move it.
- **`agent_token` via `.bearer_auth()` only, never logged** (T-04-01 pattern) — applies to the new
  version-check call if it's authenticated.

### Integration Points
- `main.rs` background-task spawn block → new update-check tokio task (startup + ~6h poll).
- New `check_version()` in `noren_client.rs` → `GET {base_url}/api/agent/version`.
- Update task → download artifact → `verify_sha256()` → Velopack stage → `UserEvent::UpdateStaged`
  → `tray_runtime` status line + one-shot toast.
- Next launch → existing `VelopackApp::build().run()` applies the staged update (SC-3).
- CI (Phase 3 Windows `vpk pack` + conditional `signtool` job) → extended to produce/publish the
  update package + surface `version`/`downloadUrl`/`sha256` (D-06).
</code_context>

<specifics>
## Specific Ideas

- **PT-BR, non-technical copy** for the owner-facing signal (audience = restaurant owner, not an
  admin): status line **"Atualização pronta — será aplicada ao reiniciar"**; toast **"Brevly Print:
  atualização pronta. Será aplicada no próximo reinício."** Final wording is Claude's discretion,
  but keep it plain and reassuring — never technical, never alarming.
- **Silence on failure is deliberate** — a failed version-check or a SHA256 mismatch produces **no**
  owner-facing error. The current version keeps running; the owner never learns an update infra
  hiccup happened. Only *success* (update ready) is surfaced.
- **The tray icon color is reserved for connection health** — update state never changes it.
</specifics>

<deferred>
## Deferred Ideas

- **Immediate / idle-time self-restart to apply an update sooner** — v1 applies strictly on the
  next natural reboot/login (D-05). A future enhancement could detect an idle window (no jobs for N
  minutes) and self-restart to apply sooner. Not v1.
- **OV certificate procurement + SmartScreen reputation warm-up** — external, carried from Phase 3
  D-12/D-14. Tracked as STATE todos; not a Phase 7 code deliverable.
- **Branded tray artwork** — deferred from Phase 3 D-05; not pulled forward.
- **Staged / percentage rollouts or a Noren-side kill-switch for a bad release** — the custom
  `/api/agent/version` endpoint *could* later gate rollouts per-tenant or pause a bad version, but
  v1 just serves the latest `{version, downloadUrl, sha256}`. Note for the Noren backlog.
- **Update channels (beta/stable)** — single stable channel in v1.

None of the above expanded Phase 7 scope — discussion stayed within the auto-update /
distribution boundary.

</deferred>

---

*Phase: 07-auto-update-distribution-polish*
*Context gathered: 2026-07-16*
