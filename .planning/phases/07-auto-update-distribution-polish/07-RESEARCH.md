# Phase 7: Auto-Update + Distribution Polish — Research

**Researched:** 2026-07-16
**Domain:** Velopack Rust SDK v1.2.0 · SHA256 integrity · Background tokio task · CI publish loop
**Confidence:** MEDIUM-HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Noren's `GET /api/agent/version` is the authoritative version signal; Velopack remains
  the apply mechanism. The agent polls the endpoint and compares `version` to
  `env!("CARGO_PKG_VERSION")`. `downloadUrl` points at the Velopack update package (`.nupkg`).
  We do NOT require Noren to host a full `releases.*.json` feed — the custom endpoint IS the
  feed; the agent bridges to Velopack for the actual apply.
- **D-02:** Explicit, manual SHA256 check is the authoritative DIST-03 gate. After downloading,
  compute SHA256 and compare (case-insensitive hex) against `sha256` from the endpoint. Mismatch
  → abort; do not stage, do not invoke Velopack, do not touch the running agent. No toast on
  mismatch (infra problem, not owner-facing). Add `sha2 = "0.10"` (RustCrypto) — pure function
  `verify_sha256(bytes, expected_hex) -> Result<()>` — Linux-unit-testable.
- **D-03:** Check on startup (a few seconds after tray up, off the print path), then poll ~6h.
  Failure is silent (log + next tick). Same tokio-task + `EventLoopProxy` pattern as other tasks.
- **D-04:** Quiet two-tier signal. Status line: `"Atualização pronta — será aplicada ao reiniciar"`.
  One-shot toast: `"Brevly Print: atualização pronta. Será aplicada no próximo reinício."` Do NOT
  change tray icon color (reserved for connection health).
- **D-05:** Apply strictly on next natural launch via the already-wired Velopack bootstrapper. No
  forced restart.
- **D-06:** Extend the Phase 3 CI (`vpk pack` + conditional `signtool`) to produce the update
  package and surface `version`/`downloadUrl`/`sha256` for Noren. OV cert stays an external
  blocker — do not procure it.
- **D-07:** Update module is `#[cfg(windows)]`-gated for Velopack apply + toast. Decision logic
  (`check_for_update`, `verify_sha256`) is pure and unit-tested on Linux.

### Claude's Discretion

- Exact Velopack Rust API for local-artifact staging vs. feed-directory fallback — D-01.
- Whether `GET /api/agent/version` needs `.bearer_auth()` — default to bearer for consistency.
- Poll interval tuning (default ~6h) and startup-check delay.
- Exact PT-BR copy for status line + toast — D-04.
- `sha2` version + whether already transitively present — D-02.
- CI job structure for producing/publishing the update package — D-06.

### Deferred Ideas (OUT OF SCOPE)

- Immediate / idle-time self-restart to apply sooner (v2).
- OV certificate procurement + SmartScreen reputation warm-up (external blocker, tracked in STATE).
- Branded tray artwork (deferred from Phase 3).
- Staged / percentage rollouts or Noren-side kill-switch.
- Update channels (beta/stable) — single stable channel in v1.
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| DIST-02 | Auto-update — agent downloads and installs new version automatically on next restart, no owner action | Velopack `wait_exit_then_apply_updates` with `restart=false` stages for next natural boot; bootstrapper applies on next `main()` call |
| DIST-03 | Integrity verification (SHA256) of the binary before applying any update | `sha2` crate (already transitively present via `velopack`) — pure `verify_sha256(bytes, expected_hex)` function, Linux-testable |
</phase_requirements>

---

## Summary

Phase 7 adds the check → download → verify → stage pipeline that feeds the already-wired
Velopack bootstrapper (`velopack::VelopackApp::build().run()` — first call in `main()`). The
phase has two interlocking concerns: (1) bridging the custom Noren version endpoint to the
Velopack apply mechanism, and (2) closing the CI release/publish loop so producing a signed,
update-ready artifact is one workflow run.

**The central architectural question** — whether Velopack's `UpdateManager` can consume an
arbitrary artifact URL or strictly requires a Velopack feed directory — has a definitive answer:
`UpdateManager::new()` requires a valid Velopack installation (returns `Err` otherwise), and its
`HttpSource` expects a `releases.{channel}.json` feed at the base URL. Because the project's
custom `/api/agent/version` endpoint returns a plain `{version, downloadUrl, sha256}` JSON (NOT
a Velopack feed), and because the running agent is already a Velopack-installed binary,
**two valid paths exist** — see the Architecture Patterns section for the authoritative
recommendation.

**SHA256 verification** is the highest-risk requirement (SC-2: mismatch must abort without
touching the running agent). The `sha2` crate is already transitively present in the project
(pulled by `velopack = "1"` at version 0.11.0), so no new dependency is needed. The
`verify_sha256` function is pure and Linux-testable.

**Background task wiring** follows the established tokio + `EventLoopProxy` + `UserEvent`
pattern exactly. A new `UpdateStaged` variant on `UserEvent` carries the "update ready" signal
to the event-loop thread, which updates the tray status line and fires a one-shot toast (reusing
the Phase 6 `tauri-winrt-notification` infra already in `retry_task.rs`).

**Primary recommendation:** Use the Noren custom endpoint as the version gate + sha256 source;
host standard Velopack feed files alongside the `.nupkg` at the same S3/CF path; call
`UpdateManager::new(HttpSource::new(feed_base_url), ...)` → `check_for_updates()` →
`download_updates()` → `verify_sha256()` → `wait_exit_then_apply_updates(..., silent=true, restart=false, [])`.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Version check + sha256 source | Noren backend (`GET /api/agent/version`) | — | Authoritative per D-01 |
| Semver comparison decision | Agent portable core | — | Pure logic, no Windows dep, D-07 |
| Download artifact | Agent (reqwest or Velopack SDK) | — | Either path works; SDK preferred |
| SHA256 verification | Agent portable core | — | Pure function, Linux-unit-testable, D-02 |
| Staging update for next boot | Velopack SDK (Windows-gated) | — | Only safe way to swap running EXE on Windows |
| Tray status + toast notification | Event-loop thread (tray_runtime + toast) | Update task → proxy | C2: all tray mutation on event-loop thread |
| Feed file hosting | Noren infrastructure (S3/CF) | — | Noren-backend dependency |
| CI publish loop | GitHub Actions (Phase 3 job extension) | — | D-06: one CI run after cert lands |

---

## Standard Stack

### Core — No New Crates Required

| Crate | Version (locked) | Purpose | Status |
|-------|-----------------|---------|--------|
| `velopack` | 1.2.0 | Bootstrapper (already wired) + `UpdateManager` for stage/apply | Already in `Cargo.toml` under `[target.'cfg(windows)'.dependencies]` |
| `sha2` | 0.11.0 | SHA-256 hash of downloaded artifact | **Already transitively present** via `velopack = "1"` — do NOT add it to `Cargo.toml` separately unless a different version is needed |
| `reqwest` | 0.13.x | Download artifact bytes (already shared `Client`) | Already in `[dependencies]` |
| `tokio` | 1.x | Background update-check task (already running runtime) | Already in `[dependencies]` |
| `tauri-winrt-notification` | 0.8 | One-shot "update ready" toast | Already in `[target.'cfg(windows)'.dependencies]` (Phase 6) |

### Supporting — Consider Adding

| Crate | Version | Purpose | Note |
|-------|---------|---------|------|
| `semver` | 1.0.28 | Semver comparison of `version` vs `env!("CARGO_PKG_VERSION")` | Already transitively present via `velopack`; adding it explicitly to `Cargo.toml` makes the dep intentional + version pinned |

[VERIFIED: npm registry] — slopcheck: `sha2` [OK], `semver` [OK] (both confirmed on crates.io)

### Version Verification

```bash
cargo search sha2     # → sha2 = "0.11.0"
cargo search semver   # → semver = "1.0.28"
```

Both confirmed via crates.io registry. `sha2` 0.11.0 is already locked in `Cargo.lock` (pulled
transitively by `velopack`). `semver` 1.0.28 is likewise already in `Cargo.lock` (pulled
transitively by `velopack`). No new crates need to be added to `Cargo.toml` for the update
logic itself.

---

## Package Legitimacy Audit

> Only new explicit additions are audited here. All crates listed are already transitively
> present in the project's `Cargo.lock` — no new packages are being installed.

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `sha2` | crates.io | ~8 yrs | Very high (RustCrypto org) | github.com/RustCrypto/hashes | [OK] | Approved — already transitive dep |
| `semver` | crates.io | ~10 yrs | Very high (Cargo itself uses it) | github.com/dtolnay/semver | [OK] | Approved — already transitive dep |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

---

## Architecture Patterns

### System Architecture Diagram

```
main() startup
│
├── VelopackApp::build().run()  ← [bootstrapper: applies staged update on next launch]
│
└── if is_runtime {
    │
    ├── spawn(run_pusher_loop)
    ├── spawn(run_print_worker)
    ├── spawn(run_retry_poll_loop)
    └── spawn(run_update_check_loop)  ← NEW Phase 7
              │
              ├── [startup delay: ~10s after tray up]
              │
              └── loop every ~6h:
                    │
                    ├── GET /api/agent/version  (noren_client::check_version)
                    │   └── {version, downloadUrl, sha256}
                    │
                    ├── check_for_update(current_ver, version) → UpdateDecision
                    │   └── [PURE, Linux-testable]
                    │
                    ├── if UpdateAvailable:
                    │   ├── download bytes via reqwest
                    │   ├── verify_sha256(bytes, sha256) → Result<()>
                    │   │   └── [PURE, Linux-testable]
                    │   │
                    │   ├── on MISMATCH: log + abort (no staging, no toast)
                    │   │
                    │   └── on MATCH (Windows-only):
                    │       ├── UpdateManager::new(HttpSource::new(feed_url))
                    │       ├── check_for_updates() → UpdateInfo
                    │       ├── download_updates(&info, None)
                    │       ├── wait_exit_then_apply_updates(&asset, true, false, [])
                    │       └── proxy.send_event(UserEvent::UpdateStaged)
                    │
                    └── [on failure: log + continue loop, no toast]

UserEvent::UpdateStaged → event_loop thread:
    ├── tray_runtime.set_status_line("Atualização pronta — será aplicada ao reiniciar")
    └── show_update_ready_toast()  [#[cfg(windows)] — once only]
```

### Recommended Project Structure

```
src/
├── update/
│   ├── mod.rs              # pub use; re-exports public surface
│   ├── check.rs            # check_for_update() pure fn + UpdateDecision enum (portable)
│   ├── verify.rs           # verify_sha256() pure fn (portable, Linux-testable)
│   └── apply.rs            # #![cfg(windows)] — UpdateManager stage + toast
├── noren_client.rs         # + check_version() fn (new, mirrors fetch_pending_jobs pattern)
├── main.rs                 # + UserEvent::UpdateStaged variant + spawn(run_update_check_loop)
└── tray_runtime.rs         # + set_update_status_line() or extend apply_health pattern
```

### Pattern 1: Velopack Feed-Bridge (Recommended Path for D-01)

**The Central Question Answered:**
`UpdateManager::new()` requires the app to be running as a valid Velopack installation (returns
`Err` when not installed — e.g., dev builds). [CITED: docs.rs/velopack/1.2.0/velopack/struct.UpdateManager.html]

`HttpSource` expects the base URL to have `releases.{channel}.json` (and `.nupkg` files) at
that path. [CITED: docs.rs/velopack/1.2.0/velopack/sources/struct.HttpSource.html]

**Recommended approach (D-01 resolution):**

1. Noren backend hosts BOTH the custom endpoint AND the standard Velopack feed files at the same
   S3/CF path. `vpk pack` already produces `releases.win.json` + `brevly-print-{ver}-full.nupkg`
   in the `--outputDir`. Upload all of them. The custom endpoint (`/api/agent/version`) serves as
   the version gate and supplies the authoritative SHA256 for D-02; `HttpSource` points at the
   same directory for the SDK's own feed consumption.

2. The agent uses `/api/agent/version` FIRST to decide whether to update at all (D-01 version
   gate). Only if an update is warranted does it instantiate `UpdateManager` and let the SDK
   consume the feed. This avoids an extra round-trip on the happy path (no update).

3. After `download_updates()`, the agent runs the manual `verify_sha256()` against the `sha256`
   from the Noren endpoint (D-02 belt-and-suspenders). Only on SHA256 match does it call
   `wait_exit_then_apply_updates`.

**Why NOT the "pure custom-download-then-stage" path:**
Velopack has no public API to "stage a locally-downloaded `.nupkg` without going through
`UpdateManager`". The stage/apply mechanism is internal to the SDK and invoked only through
`wait_exit_then_apply_updates` or `apply_updates_and_restart`. There is no
`UpdateManager::stage_local_package(path)` method. [CITED: docs.rs/velopack/1.2.0] Attempting
to hand-roll the staging location would replicate exactly the problem Velopack exists to solve
(safely swapping a running EXE). The feed-bridge approach adds zero complexity to the Noren
backend (it's a static file upload, already part of the `vpk upload` CI step).

```rust
// Source: docs.rs/velopack/1.2.0/velopack/struct.UpdateManager.html
// (Windows-gated — called only from apply.rs after verify_sha256 passes)

use velopack::{UpdateManager, sources::HttpSource};

let feed_base_url = /* derive from downloadUrl: strip filename, keep directory */;
let um = UpdateManager::new(HttpSource::new(feed_base_url), None, None)?;
// check_for_updates() fetches {feed_base_url}/releases.win.json
let update = match um.check_for_updates()? {
    velopack::UpdateCheck::UpdateAvailable(info) => info,
    _ => return Ok(()), // no update per SDK (should match our endpoint check)
};
// download_updates stages the .nupkg to Velopack's packages dir
um.download_updates(&update, None)?;
// wait_exit_then_apply_updates launches the updater process and tells it to wait for
// graceful exit. silent=true (no UI), restart=false (do not relaunch — SC-3: next natural boot)
// The updater process waits up to 60s for this process to exit, then applies and optionally restarts.
// With restart=false: apply happens on next manual launch (VelopackApp::build().run() picks it up).
um.wait_exit_then_apply_updates(&update.to_apply, true, false, std::iter::empty::<&str>())?;
```

[ASSUMED] — The exact generic parameter `to_apply` field name on `UpdateInfo` was not directly
confirmed in docs.rs output; the `wait_exit_then_apply_updates` signature requires
`A: AsRef<VelopackAsset>` so the caller likely passes `&update_info.to_apply` or the
`VelopackAsset` from `UpdateInfo`. Planner must confirm against the 1.2.0 source/docs.

### Pattern 2: `check_version()` in `noren_client.rs` (mirrors `fetch_pending_jobs`)

```rust
// Source: existing pattern in src/noren_client.rs (fetch_pending_jobs)
// New function follows the same shape.

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionResponse {
    pub version: String,
    pub download_url: String,
    pub sha256: String,
}

pub async fn check_version(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,     // bearer auth — consistent with all other endpoints
) -> anyhow::Result<VersionResponse> {
    let url = format!("{base_url}/api/agent/version");
    let resp = client
        .get(&url)
        .bearer_auth(agent_token)   // T-02-02: token never logged
        .send()
        .await
        .context("check_version: HTTP transport error")?;
    match resp.status().as_u16() {
        200 => resp.json::<VersionResponse>().await.context("check_version: parse error"),
        status => anyhow::bail!("check_version: unexpected status {status}"),
    }
}
```

### Pattern 3: `verify_sha256` — Pure, Linux-Testable

```rust
// Source: sha2 0.11.0 — docs.rs/sha2/0.11.0/sha2/
// Portable: no #[cfg(windows)]. Must be in a portable module (e.g., src/update/verify.rs).

use sha2::{Sha256, Digest};

/// Verify that `bytes` hashes to `expected_hex` (case-insensitive SHA-256 hex).
///
/// Returns `Ok(())` on match. Returns `Err` on mismatch, wrong length, or malformed hex.
/// Pure function — no I/O, no Windows deps — Linux unit-testable.
pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> anyhow::Result<()> {
    let hash = Sha256::digest(bytes);
    let computed = hex::encode(hash);  // or format with {:02x}
    if computed.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        anyhow::bail!(
            "SHA256 mismatch: expected {expected_hex}, got {computed}"
        )
    }
}
```

Note: `hex` crate may need to be added, OR use `format!("{:02x}", byte)` in a loop (no extra
dep). The `base64` crate is already present; `hex` is not in `Cargo.lock`. The pure-loop
approach avoids adding a dep:

```rust
let computed: String = hash.iter().map(|b| format!("{b:02x}")).collect();
```

### Pattern 4: `check_for_update` — Pure Decision Function

```rust
// Portable — src/update/check.rs. No Velopack/Windows imports.
// `semver` is already transitively present via velopack.

use semver::Version;

pub enum UpdateDecision {
    UpToDate,
    UpdateAvailable,
    Err(String),
}

/// Compare `current` (from env!("CARGO_PKG_VERSION")) with `remote` (from endpoint).
/// Pure: no I/O, no Windows dep. Linux-unit-testable.
pub fn check_for_update(current: &str, remote: &str) -> UpdateDecision {
    let cur = match Version::parse(current) {
        Ok(v) => v,
        Err(e) => return UpdateDecision::Err(format!("invalid current version: {e}")),
    };
    let rem = match Version::parse(remote) {
        Ok(v) => v,
        Err(e) => return UpdateDecision::Err(format!("invalid remote version: {e}")),
    };
    if rem > cur { UpdateDecision::UpdateAvailable } else { UpdateDecision::UpToDate }
}
```

### Pattern 5: Background Update Task — Sibling to Pusher/Retry Tasks

```rust
// In main.rs, inside `if is_runtime { ... }` spawn block.
// Mirrors pusher/retry task structure exactly (D-03).

let proxy_for_update = event_loop.create_proxy();
let update_token    = agent_token.clone();
let update_base_url = worker_base_url.clone();
let update_http     = http.clone();

rt_handle.spawn(async move {
    run_update_check_loop(update_http, update_base_url, update_token, proxy_for_update).await;
});
```

The `run_update_check_loop` function:
- Sleeps ~10s on startup (tray must be visible, Pusher connect in progress).
- Runs the check/download/verify/stage sequence.
- On `UserEvent::UpdateStaged` arrival on the event-loop thread: update the tray status line
  + fire the one-shot toast. Use a flag (e.g., `AtomicBool` or channel) so the toast only fires
  once per session even if the loop runs again at the 6h mark.
- On any error: `eprintln!` + continue; never propagate.

### Pattern 6: `UserEvent::UpdateStaged` and Tray Update

```rust
// In src/main.rs — extend the existing UserEvent enum:
enum UserEvent {
    #[cfg(windows)] TrayIconEvent(tray_icon::TrayIconEvent),
    #[cfg(windows)] MenuEvent(tray_icon::menu::MenuEvent),
    HealthChanged(HealthState),
    UpdateStaged,          // NEW — Phase 7
}

// In App::user_event():
UserEvent::UpdateStaged => {
    #[cfg(windows)]
    if let Some(rt) = &self.tray_runtime {
        rt.set_update_status();   // updates the disabled status line text
    }
    // show_update_ready_toast() — one-shot, #[cfg(windows)] inside
    show_update_ready_toast();
}
```

```rust
// In src/tray_runtime.rs — add method to TrayRuntime:
pub fn set_update_status(&self) {
    self.menu_items.status.set_text(
        "Atualização pronta — será aplicada ao reiniciar"
    );
    let _ = self.tray.set_tooltip(Some(
        "Brevly Print — Atualização pronta"
    ));
}
```

```rust
// Update toast — same pattern as show_print_failure_toast() in retry_task.rs:
fn show_update_ready_toast() {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;
        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title("Brevly Print")
            .text1("Atualização pronta. Será aplicada no próximo reinício.")
            .show();
    }
    #[cfg(not(windows))]
    eprintln!("[brevly-print] Update staged (Linux: stderr only)");
}
```

### Pattern 7: `wait_exit_then_apply_updates` — Stage Without Restart

**Key distinction** between the two apply methods [CITED: docs.rs/velopack/1.2.0]:

| Method | Behavior | Use for Phase 7? |
|--------|----------|-----------------|
| `apply_updates_and_restart(&asset)` | Exits app immediately, applies, relaunches now | NO — violates SC-1 (interrupts printing) |
| `wait_exit_then_apply_updates(&asset, silent, restart, args)` | Launches updater process; updater waits up to 60s for graceful exit; with `restart=false`, does NOT relaunch | YES — call with `silent=true, restart=false` |

With `restart=false, silent=true`: the updater process is spawned and waits for the agent to exit.
The agent does NOT exit (it keeps running — printing continues). When the agent eventually exits
on a natural Windows reboot/login, the updater applies the staged `.nupkg`. On the next
`main()` call, `VelopackApp::build().run()` detects the staged update and applies it before
`main()` logic runs.

[ASSUMED] — Exact behavior of `wait_exit_then_apply_updates` with `restart=false` when the
60-second wait expires was not explicitly confirmed in official docs. The documented behavior
says the updater "will only wait for 60 seconds before giving up." It is unclear whether
"giving up" means the update is lost or remains staged. On a long-running restaurant PC, the
60s timeout is a real concern: the updater is spawned immediately when the method is called,
and the agent is NOT expected to exit within 60s. Planner must investigate: does the staged
`.nupkg` persist even after the updater times out? If not, the recommended approach is to
call `wait_exit_then_apply_updates` only at natural shutdown (e.g., in a Drop impl or on
`event_loop.exit()`), NOT immediately after download. See Anti-Patterns below.

### Anti-Patterns to Avoid

- **Calling `wait_exit_then_apply_updates` immediately from the background task:** The updater
  process starts a 60-second countdown immediately. On a running-all-day restaurant PC, the
  agent won't exit for hours. If the updater times out and the stage is lost, the update never
  applies. Instead: either (a) the background task downloads + verifies + signals `UpdateStaged`,
  and the actual `wait_exit_then_apply_updates` call happens only when the event loop is exiting
  (e.g., on "Sair" menu action or OS shutdown) — OR (b) use `velopack`'s internal staging
  directory directly through the SDK's `download_updates()` which stages the `.nupkg` to disk;
  then call `wait_exit_then_apply_updates` at shutdown. [ASSUMED — needs spike validation]

- **Calling `apply_updates_and_restart` from the update task:** This exits the process immediately
  — violates SC-1. Never use it in Phase 7.

- **Adding a new top-level `sha2` dependency with version "0.10":** `sha2` 0.11.0 is already
  in `Cargo.lock` via `velopack`. Adding `sha2 = "0.10"` creates two major-version copies.
  Instead, either (a) use `sha2` without specifying it in `Cargo.toml` (rely on transitive) or
  (b) explicitly add `sha2 = "0.11"` to lock the known version.

- **Logging the `sha256` mismatch with the received hash in a way that leaks the `agent_token`:**
  The mismatch log should include the expected vs. computed hex but must never mention the token.
  The token is in the HTTP response header path, not the body, but pattern is established: no
  secrets in logs.

- **Making `check_for_update()` / `verify_sha256()` depend on any `#[cfg(windows)]` type:**
  Both must compile and pass tests on Linux with `cargo test` (no Windows target). Keep them in
  a portable module with no Windows imports.

- **Double-toast:** If the 6h poll runs again after an update is already staged, do NOT fire
  another toast. Use a flag/channel to gate the `show_update_ready_toast` call to once per session.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Swapping a locked running EXE on Windows | Custom file-swap / rename / restart logic | `velopack` SDK `wait_exit_then_apply_updates` | The OS locks the EXE while it's running; Velopack uses a separate updater process with the right Win32 sequence (CLAUDE.md explicitly rejects `self_update` for this) |
| Update feed JSON format | Custom manifest parser or new endpoint format | Standard Velopack `releases.win.json` (produced by `vpk pack`) | Reuses the same artifact the SDK already knows how to parse; hosting is a static file upload |
| Hex encoding of SHA256 | Custom nibble-to-hex loop | `sha2::Sha256::digest()` + `format!("{b:02x}")` per byte OR `hex` crate | One-liners; the RustCrypto ecosystem is proven |
| Version comparison | String split + integer compare | `semver::Version::parse()` + `>` | Handles pre-release, patch, build metadata correctly; already in the tree |
| Delta update calculation | Download + patch logic | `vpk pack --delta BestSpeed` | Velopack does this automatically when a previous release exists in `--outputDir` |

**Key insight:** The only truly custom work in this phase is the HTTP fetch of
`/api/agent/version`, the SHA256 check, and the wiring. The download, stage, apply, and feed
management are entirely Velopack SDK + `vpk` CLI.

---

## Common Pitfalls

### Pitfall 1: `wait_exit_then_apply_updates` 60-Second Timeout on Long-Running Process

**What goes wrong:** The method launches an external updater process that waits for the agent
to exit. Restaurant PCs run for hours. If the updater times out, the stage may be lost and the
update silently never applies on the next boot.

**Why it happens:** `wait_exit_then_apply_updates` is designed for "apply shortly after
download" flows, not "apply at some indeterminate future reboot" flows.

**How to avoid:** Call `wait_exit_then_apply_updates` only at process-exit time (when the user
clicks "Sair", or on natural OS shutdown hook), NOT immediately from the background task. The
background task does check → download → verify → signal `UpdateStaged`. The actual
`wait_exit_then_apply_updates` call goes in the "Sair" handler or a `Drop` impl on `App`.
[ASSUMED — needs spike validation to confirm the `.nupkg` downloaded by `download_updates()`
persists on disk independently of the updater process, allowing `wait_exit_then_apply_updates`
to be called later]

**Warning signs:** Update is staged (tray shows "Atualização pronta") but the new version
never appears after reboot.

### Pitfall 2: `UpdateManager::new()` Fails on Dev Builds ("Not Installed" Error)

**What goes wrong:** `UpdateManager::new()` returns `Err` when the app is not running as a
Velopack-installed binary. In a dev build (cargo run) or CI, this always errors.
[CITED: docs.rs/velopack/1.2.0/velopack/struct.UpdateManager.html]

**Why it happens:** Velopack stores installation metadata in the app directory. A raw cargo
binary has no Velopack installation context.

**How to avoid:** Gate the `UpdateManager::new()` call behind `#[cfg(windows)]` AND wrap it in
a `match`. On `Err`, log and return — do not panic. For testing, use a locally-installed dev
package (`vpk pack` → `Setup.exe` → install locally → run from install location) or a
`VelopackLocatorConfig` if the Rust SDK supports it. The C# SDK has `TestVelopackLocator`
but **the Rust SDK does NOT have an equivalent test locator** — Rust-language integration
tests for the Velopack apply path require a real Velopack installation.
[CITED: docs.velopack.io/integrating/testing — "for other languages (JS, Python, Rust, C++)
there is no built-in test locator"]

**Warning signs:** The update-check tokio task immediately logs an error and exits on every
startup without ever reaching `check_for_updates()`.

### Pitfall 3: SHA256 Case-Sensitivity Mismatch

**What goes wrong:** Noren endpoint may return uppercase hex (`A1B2C3...`), sha2 produces
lowercase. String equality check fails even when the hash is correct.

**Why it happens:** SHA256 hex has no canonical case by spec.

**How to avoid:** `verify_sha256` must use `eq_ignore_ascii_case()` for the comparison. Unit
test with both uppercase and lowercase expected values.

**Warning signs:** SC-2 test fails even when the correct bytes are provided.

### Pitfall 4: `sha2` Version Conflict

**What goes wrong:** Adding `sha2 = "0.10"` to `Cargo.toml` when `sha2 = "0.11.0"` is already
in `Cargo.lock` creates two incompatible major versions. The `Digest` trait is different between
0.10 and 0.11, causing type errors.

**Why it happens:** RustCrypto changed the `Digest` trait major version between 0.10 and 0.11.

**How to avoid:** Check `Cargo.lock` first (already done: 0.11.0 is present). Use `sha2 =
"0.11"` if an explicit dep is added, or rely on the transitive dep without an explicit entry.
The `Sha256::digest(bytes)` one-shot API works the same way in both 0.10 and 0.11.

### Pitfall 5: Noren Endpoint Auth Ambiguity

**What goes wrong:** `/api/agent/version` may be intentionally unauthenticated (a public
version manifest), but the agent sends `Bearer` auth by default. This is not an error if the
endpoint accepts optional auth, but if the Noren backend requires auth, the agent will fail
silently (returns 401, which the current error path logs as "unexpected status 401").

**Why it happens:** CONTEXT.md marks auth as "Claude's Discretion" — no explicit lock on whether
the version endpoint requires auth.

**How to avoid:** Default to `.bearer_auth(agent_token)` (consistent with all other agent
endpoints per CONTEXT.md D-07). Flag this explicitly for Noren backend team: if the endpoint is
to be public (unauthenticated), the agent needs a conditional branch or a separate unauth
variant. [ASSUMED — backend auth requirement not locked]

### Pitfall 6: Double-Download (Velopack SDK Downloads the `.nupkg` After Manual Download)

**What goes wrong:** The D-01 flow downloads the artifact manually (to run SHA256 verify), then
calls `UpdateManager` which downloads it again via `download_updates()`. Two downloads, double
bandwidth, and the manually-downloaded bytes are thrown away.

**Why it happens:** The Velopack SDK has no API to inject already-downloaded bytes; it manages
its own packages directory.

**How to avoid:** This is an accepted tradeoff of the feed-bridge approach. The `.nupkg` for a
print-agent binary is small (~10 MB max). Two downloads are acceptable. Alternatively: skip the
manual download entirely, let Velopack SDK download via `download_updates()`, then compute
SHA256 over the staged `.nupkg` file on disk. [ASSUMED — staging path location is
`%LocalAppData%\{AppId}\packages\` — planner should confirm via VelopackLocatorConfig or test]

**Warning signs:** Logs show two HTTP GETs to the same `downloadUrl`.

### Pitfall 7: `vpk pack` Needs Previous Release for Delta

**What goes wrong:** On the first publish run (v0.2.0 release), delta generation fails or
produces a zero-byte file because there's no previous `.nupkg` in `--outputDir`.

**Why it happens:** Velopack generates `delta.nupkg` only when a previous release exists in
the output directory. First run has no previous release.

**How to avoid:** On first publish: use `--delta None` or `--noPortable` / accept no delta. From
v0.2.0 onwards, the CI pipeline should download the previous `releases.win.json` + `.nupkg`
before running `vpk pack` so delta is generated. Add `--delta BestSpeed` flag.

---

## Code Examples

### Complete update task skeleton

```rust
// src/update/mod.rs — portable, no Windows deps
// Source: based on established project pattern (pusher/client.rs, retry_task.rs)

pub async fn run_update_check_loop(
    http: reqwest::Client,
    base_url: String,
    agent_token: String,
    proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>,
) {
    // D-03: startup delay — wait for tray + Pusher connect to be underway
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    let mut update_staged = false;

    loop {
        if !update_staged {
            match try_check_and_stage(&http, &base_url, &agent_token).await {
                Ok(true) => {
                    let _ = proxy.send_event(crate::UserEvent::UpdateStaged);
                    update_staged = true; // don't re-toast
                }
                Ok(false) => {} // up to date or mismatch — silent
                Err(e) => eprintln!("[brevly-print] Update check failed: {e:#}"),
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(6 * 60 * 60)).await;
    }
}

async fn try_check_and_stage(
    http: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
) -> anyhow::Result<bool> {
    use crate::noren_client::check_version;
    use crate::update::{check::check_for_update, check::UpdateDecision, verify::verify_sha256};

    let ver_info = check_version(http, base_url, agent_token).await?;

    let current = env!("CARGO_PKG_VERSION");
    match check_for_update(current, &ver_info.version) {
        UpdateDecision::UpToDate => return Ok(false),
        UpdateDecision::Err(e) => anyhow::bail!("version parse: {e}"),
        UpdateDecision::UpdateAvailable => {}
    }

    // Download artifact for SHA256 verify (D-02)
    let bytes = http.get(&ver_info.download_url).send().await?.bytes().await?;
    verify_sha256(&bytes, &ver_info.sha256)?;

    // Stage via Velopack SDK (Windows-only)
    #[cfg(windows)]
    {
        crate::update::apply::stage_update(&ver_info.download_url)?;
    }
    #[cfg(not(windows))]
    eprintln!("[brevly-print] Update staged (Linux stub — no Velopack apply)");

    Ok(true)
}
```

### CI publish step (GitHub Actions, extending Phase 3 job)

```yaml
# Extending the existing Windows CI job (Phase 3 D-12).
# Source: docs.velopack.io/reference/cli (vpk pack flags) + Phase 3 CI pattern

- name: Pack Velopack release
  run: |
    vpk pack \
      --packId brevly-print \
      --packVersion "${{ steps.version.outputs.version }}" \
      --packDir target/release \
      --mainExe brevly-print.exe \
      --outputDir Releases \
      --channel win \
      --delta BestSpeed

- name: Sign installer (conditional — OV cert gate from Phase 3 D-12)
  if: env.CODESIGN_PFX_BASE64 != ''
  env:
    CODESIGN_PFX_BASE64: ${{ secrets.CODESIGN_PFX_BASE64 }}
    CODESIGN_PFX_PASSWORD: ${{ secrets.CODESIGN_PFX_PASSWORD }}
  run: |
    # signtool sign ... (existing Phase 3 step, unchanged)

- name: Extract update artifact info for Noren endpoint
  run: |
    # vpk produces assets.win.json listing the artifacts + SHA256
    # Surface version, downloadUrl, sha256 as CI job outputs
    $asset = Get-Content Releases/assets.win.json | ConvertFrom-Json
    $fullPkg = $asset.assets | Where-Object { $_.type -eq "Full" }
    echo "version=${{ steps.version.outputs.version }}" >> $env:GITHUB_OUTPUT
    echo "filename=$($fullPkg.fileName)" >> $env:GITHUB_OUTPUT
    echo "sha256=$($fullPkg.sha256)" >> $env:GITHUB_OUTPUT

- name: Upload to S3/Cloudflare (Noren backend dependency)
  # vpk upload s3 --bucket ... (Noren team implements this step)
  # This step runs only after OV cert and hosting are configured.
  # For now, the artifacts are available as CI job artifacts.
  run: echo "Upload step: Noren backend dependency (see D-06)"
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `self_update` crate for Windows binary replacement | Velopack (separate updater process) | — | `self_update` cannot replace the locked running EXE; Velopack solves this via an out-of-process updater |
| EV certificates bypass SmartScreen | OV = EV for SmartScreen reputation (both build via download volume) | March 2024 | EV no longer instant-clean; see CLAUDE.md signing notes |
| SHA1 for package integrity in Velopack | SHA256 supported (preferred over SHA1 if present) | velopack ~1.x | `releases.win.json` now includes SHA256 field per VelopackAsset |
| `apply_updates_and_restart` for all update flows | `wait_exit_then_apply_updates` for "apply on next boot" | — | Separation of concerns: immediate restart vs. deferred apply |

**Deprecated/outdated:**
- `pusher-rs` / `pusher` (WillSewell): unsupported — already rejected in CLAUDE.md (irrelevant here but documents ecosystem context).
- `self_update` crate: explicitly rejected for Windows binary replacement (CLAUDE.md).

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `wait_exit_then_apply_updates` with `restart=false` + 60s timeout: the staged `.nupkg` persists on disk even after the updater process times out, allowing the bootstrapper to apply it on the next natural launch | Architecture Patterns §Pattern 7, Pitfall 1 | If the stage is lost after 60s, updates never apply on long-running PCs — the entire D-05 "next boot" strategy fails |
| A2 | The exact field on `UpdateInfo` that holds the `VelopackAsset` to pass to `wait_exit_then_apply_updates` (likely `update_info.to_apply` or similar) | Architecture Patterns §Pattern 1 | Wrong field name causes a compile error — trivially fixed but must be confirmed against 1.2.0 docs/source |
| A3 | `GET /api/agent/version` requires Bearer auth (D-07 discretion defaulted to bearer) | Architecture Patterns §Pattern 2, Pitfall 5 | If the endpoint is intentionally public, the bearer token is sent unnecessarily (harmless but unnecessary); if the backend rejects auth on a public endpoint, the check fails |
| A4 | The `sha2 = "0.11.0"` transitive dep (via `velopack`) is sufficient — no explicit `Cargo.toml` entry needed for `sha2::Sha256` to be importable in `src/update/verify.rs` | Standard Stack | In Rust, transitive deps are usable but considered bad practice without an explicit `Cargo.toml` entry; adding `sha2 = "0.11"` explicitly is safer |
| A5 | `assets.win.json` produced by `vpk pack` contains a `sha256` field for the full package (used in CI step to surface the value for Noren's endpoint) | Code Examples §CI | If `assets.win.json` lacks SHA256, the CI step needs to compute it manually (`shasum -a 256` on the `.nupkg`) |
| A6 | The Noren version endpoint `{version, downloadUrl, sha256}` uses camelCase JSON keys | Architecture Patterns §Pattern 2 | Wrong case → serde deserialization fails silently (returns error) → update check always fails |

**If this table is empty:** Not applicable — several assumptions require spike validation.

---

## Open Questions

1. **`wait_exit_then_apply_updates` staging persistence past 60s timeout**
   - What we know: The method spawns an updater process that waits up to 60s for graceful exit.
   - What's unclear: Does the staged `.nupkg` (downloaded by `download_updates()`) persist
     independently on disk even after the updater process times out? Or does the updater clean
     it up on timeout?
   - Recommendation: **Spike required.** Install a dev build via `vpk pack` + `Setup.exe`, call
     `download_updates()` + `wait_exit_then_apply_updates(..., restart=false)`, let 60s elapse
     without exiting, then check the Velopack packages directory
     (`%LocalAppData%\brevly-print\packages\`). If the `.nupkg` is there, the bootstrapper
     will apply it on next launch regardless of the updater process outcome.

2. **Whether to call `wait_exit_then_apply_updates` immediately vs. at shutdown**
   - What we know: Calling immediately spawns a waiting updater process. Restaurant agent runs
     for hours. 60s timeout makes immediate call questionable.
   - What's unclear: If the stage persists (OQ1 answer = yes), calling at shutdown is simpler
     and cleaner. If not, a workaround is needed.
   - Recommendation: Resolve OQ1 first. If stage persists, the background task calls
     `download_updates()` only, and `wait_exit_then_apply_updates` is called on process exit
     (e.g., in the "Sair" handler or a shutdown hook).

3. **`/api/agent/version` authentication requirement**
   - What we know: CONTEXT.md marks this as Claude's Discretion, defaulting to bearer auth.
   - What's unclear: Noren backend team's intent (public manifest vs. tenant-scoped).
   - Recommendation: Default to bearer auth in code; add a comment flagging the option to make
     it unauthenticated. Flag for Noren backend team at the start of Phase 7 execution.

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|-------------|-----------|---------|----------|
| `vpk` CLI | D-06 CI publish step | [ASSUMED not on dev Linux] | — | CI only (Windows runner) — install via `npm i -g @velopack/vpk` or download binary |
| Velopack-installed binary | `UpdateManager::new()` in Windows integration test | Requires local install | — | Install via `vpk pack` → `Setup.exe` on a Windows VM/machine |
| Windows (for integration test) | Velopack apply path | Not available on Linux CI | — | Linux CI tests only pure functions; Windows CI tests the full apply path |
| S3/Cloudflare hosting | D-06 upload step | Noren-side blocker | — | Use CI artifacts as interim; actual upload is Noren dependency |

**Missing dependencies with no fallback:**
- Windows machine/VM for `UpdateManager` integration test and full update flow verification.
  The pure logic (version compare, SHA256) tests on Linux; the apply path requires Windows.

**Missing dependencies with fallback:**
- `vpk` CLI: installable in Windows CI runner; not needed on Linux.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in (`cargo test`) — existing project tests use this |
| Config file | `Cargo.toml` `[dev-dependencies]` + `tests/` directory |
| Quick run command | `cargo test --lib` (Linux-safe, no Windows deps) |
| Full suite command | `cargo test` (Linux; Windows: `cargo test --target x86_64-pc-windows-msvc`) |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DIST-02 | Agent downloads + stages update silently, applies on next boot | Integration (Windows) | Manual / Velopack-installed binary required | ❌ Wave 0 |
| DIST-03 — match | `verify_sha256(correct_bytes, correct_hex)` returns `Ok(())` | Unit (Linux) | `cargo test --lib update::verify` | ❌ Wave 0 |
| DIST-03 — mismatch | `verify_sha256(tampered_bytes, correct_hex)` returns `Err` | Unit (Linux) | `cargo test --lib update::verify` | ❌ Wave 0 |
| DIST-03 — SC-2 abort | Mismatch: no `wait_exit_then_apply_updates` called, running agent unchanged | Unit (Linux) — mock Velopack apply | `cargo test --lib update` | ❌ Wave 0 |
| D-01 | `check_for_update("0.1.0", "0.2.0")` → `UpdateAvailable` | Unit (Linux) | `cargo test --lib update::check` | ❌ Wave 0 |
| D-01 | `check_for_update("0.2.0", "0.1.0")` → `UpToDate` | Unit (Linux) | `cargo test --lib update::check` | ❌ Wave 0 |
| D-01 | `check_for_update("0.1.0", "invalid")` → `Err` | Unit (Linux) | `cargo test --lib update::check` | ❌ Wave 0 |
| D-02 | `verify_sha256(bytes, "UPPER_HEX")` succeeds (case insensitive) | Unit (Linux) | `cargo test --lib update::verify` | ❌ Wave 0 |
| D-02 | `verify_sha256(&[], hex)` returns `Err` (wrong-length input) | Unit (Linux) | `cargo test --lib update::verify` | ❌ Wave 0 |
| D-03 | Update task does not panic on HTTP error | Unit (Linux) — mock HTTP | `cargo test --lib update` | ❌ Wave 0 |

### SC-2 (Highest-Risk Behavior) — How to Prove It

Success Criterion 2: "a mismatch aborts the update without touching the running agent."

To prove this:
1. **Unit test (Linux):** In `try_check_and_stage`, the `verify_sha256` call returns `Err`.
   Assert that (a) the function returns `Ok(false)` or `Err`, (b) no `UserEvent::UpdateStaged`
   is sent on the proxy, and (c) `stage_update()` (the Windows-gated call) is never invoked.
   Achieve (c) by having `stage_update` return an `Err` OR by running on Linux where it's a no-op
   stub — the test checks that the stub path is only reached when SHA256 passes.
2. **Windows integration test:** Provide tampered bytes (correct content + one byte flipped).
   Confirm: (a) `UpdateStaged` event is NOT received, (b) tray status line does NOT change, (c)
   next launch is still the old binary version.

### Sampling Rate

- **Per task commit:** `cargo test --lib` (Linux, quick, ~5s)
- **Per wave merge:** `cargo test` (Linux full suite, ~15s)
- **Phase gate:** Full suite green on Linux + Windows smoke (manual install + version bump test) before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src/update/mod.rs` — module skeleton
- [ ] `src/update/check.rs` — `check_for_update` + `UpdateDecision` + unit tests
- [ ] `src/update/verify.rs` — `verify_sha256` + unit tests (match / mismatch / case / malformed)
- [ ] `src/update/apply.rs` — `#![cfg(windows)]` stub + real `stage_update()` impl
- [ ] `tests/update_task_test.rs` — integration-level tests for `try_check_and_stage` using mock HTTP (or moved to `#[cfg(test)]` inside the module)

---

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | yes (version endpoint) | `.bearer_auth(agent_token)` — same as all Noren endpoints |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | yes | `semver::Version::parse()` for remote version; `expected_hex` length check in `verify_sha256` |
| V6 Cryptography | yes | `sha2::Sha256` (RustCrypto) — never hand-roll; constant-time comparison for SHA256 hex |

### Known Threat Patterns for Update Delivery Stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Tampered update binary on S3/CDN | Tampering | `verify_sha256()` against value from authenticated Noren endpoint (D-02) |
| MITM on `downloadUrl` HTTP | Tampering / Disclosure | HTTPS (reqwest + rustls, TLS validation on by default) |
| Malformed `version` field causes panic | DoS | `semver::Version::parse()` returns `Err` — handled gracefully, no panic |
| `sha256` field poisoned in Noren endpoint | Tampering | Bearer auth on the version endpoint means the attacker must compromise the Noren backend or the agent's token |
| Double-spend / replay (apply old update) | Elevation of Privilege | Velopack's version comparison in `check_for_updates()` + `semver` comparison in `check_for_update()` both gate on version ordering |
| Agent token logged in update error path | Information Disclosure | T-02-02 pattern: token only via `.bearer_auth()`, never in `eprintln!` / `format!` / `anyhow::bail!` — enforce in code review |

---

## Sources

### Primary (HIGH confidence)

- `docs.rs/velopack/1.2.0/velopack/struct.UpdateManager.html` — `UpdateManager::new`,
  `check_for_updates`, `download_updates`, `wait_exit_then_apply_updates`,
  `apply_updates_and_restart` full signatures and docstrings
- `docs.rs/velopack/1.2.0/velopack/sources/struct.HttpSource.html` — `HttpSource::new`,
  URL format (appends `/RELEASES` to base URL — or `releases.{channel}.json` per v1.2.0)
- `docs.rs/sha2/0.11.0/sha2/` — `Sha256::digest()` one-shot API, `Digest` trait
- `/home/zephyr/repos/brevly/brevly-print/Cargo.lock` — confirmed `sha2 = "0.11.0"` pulled
  transitively by `velopack`, `semver = "1.0.28"` also transitive via `velopack`
- `/home/zephyr/repos/brevly/brevly-print/src/main.rs` — existing background-task spawn
  pattern (Pusher, print worker, retry — Phase 7 task is a sibling), `UserEvent` enum,
  `EventLoopProxy` pattern
- `/home/zephyr/repos/brevly/brevly-print/src/noren_client.rs` — `check_version()` follows
  `fetch_pending_jobs()` shape exactly
- `/home/zephyr/repos/brevly/brevly-print/src/retry_task.rs:488–504` — `show_print_failure_toast()`
  pattern reused for `show_update_ready_toast()`
- `/home/zephyr/repos/brevly/brevly-print/src/tray_runtime.rs` — `TrayRuntime::apply_health`,
  `menu_items.status.set_text()` pattern for `set_update_status()`

### Secondary (MEDIUM confidence)

- `docs.velopack.io/integrating/update-sources` — SimpleWebSource / HttpSource feeds from
  `releases.{channel}.json`; S3 / HTTP static hosting confirmed
- `docs.velopack.io/packaging/overview` — `vpk pack` output files: full/delta `.nupkg`,
  `releases.win.json`, `assets.win.json`, `Setup.exe`
- `docs.velopack.io/reference/cli/content/vpk-windows` — `vpk pack` flags: `--packId`,
  `--packVersion`, `--packDir`, `--outputDir`, `--channel`, `--delta`
- `docs.velopack.io/distributing/deploy-cli` — `vpk upload s3 --bucket ... --keyId ... --secret ...`
- `docs.velopack.io/integrating/testing` — Rust has no `TestVelopackLocator`; use local install

### Tertiary (LOW confidence — needs spike)

- `wait_exit_then_apply_updates` with `restart=false` and 60s timeout behavior on long-running
  processes — inferred from docs; actual staging persistence not explicitly confirmed
- `assets.win.json` SHA256 field presence and format — inferred from GitHub issue #105 (closed
  with PR #140 adding SHA256 support to `VelopackAsset`)

---

## Metadata

**Confidence breakdown:**

| Area | Level | Reason |
|------|-------|--------|
| Standard Stack (no new deps) | HIGH | Verified in Cargo.lock; sha2 + semver already transitive |
| Velopack feed-bridge path (D-01) | MEDIUM | HttpSource URL format confirmed; feed-bridge approach logical; exact `UpdateInfo` field name needs check |
| `wait_exit_then_apply_updates` staging-without-restart | MEDIUM | Method exists and `restart=false` is documented; 60s timeout behavior on long-running process is ASSUMED |
| SHA256 verify (`verify_sha256`) | HIGH | sha2 0.11 API confirmed; one-shot `Sha256::digest()` is trivial |
| Background task wiring | HIGH | Exact pattern from existing tasks (pusher, retry) — no unknowns |
| CI publish loop | MEDIUM | `vpk pack` flags + `vpk upload` confirmed; `assets.win.json` SHA256 field assumed |
| Toast notification pattern | HIGH | Exact code from `retry_task.rs:492–504` confirmed; Phase 6 pattern proven |
| Tray status line update | HIGH | `menu_items.status.set_text()` confirmed in `tray_runtime.rs:62` |

**Research date:** 2026-07-16
**Valid until:** 2026-08-16 (stable Velopack Rust SDK; sha2/semver are stable crates)
