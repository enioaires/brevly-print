#![cfg(windows)]
//! Windows-only: Velopack UpdateManager staging.
//!
//! Compiled only when `cfg(windows)`. On Linux, `src/update/mod.rs` provides
//! a no-op stub so the surrounding logic remains testable (D-07).
//!
//! ## OQ-1 Design Decision (staging-persistence, plan 02)
//!
//! RESEARCH.md identified two candidate designs for when to call
//! `wait_exit_then_apply_updates`:
//!
//!   - **Design A (immediate):** Call `wait_exit_then_apply_updates` right after
//!     `download_updates()` in the background task. The updater process waits up to
//!     60s for graceful exit; if the `.nupkg` persists on disk after the timeout,
//!     the bootstrapper applies it on the next natural launch regardless.
//!
//!   - **Design B (deferred):** Call `wait_exit_then_apply_updates` only from the
//!     "Sair" handler / process exit so the updater's 60s window overlaps the actual
//!     exit. This requires `stage_update` to return after `download_updates` only, and
//!     a separate `apply_staged_on_exit()` to be wired to the exit path.
//!
//! **Chosen: Design A (immediate call).**
//!
//! Rationale: RESEARCH.md recommends Design A as the default; Velopack's own docs
//! indicate the staged `.nupkg` is written to the packages directory before the
//! updater process is spawned, and the bootstrapper (`VelopackApp::build().run()`) is
//! the component that actually performs the swap on next launch — not the updater
//! process itself. The 60s updater wait is for a *graceful* apply-on-current-run; the
//! staged package should persist independently.
//!
//! **CAVEAT (OQ-1, 07-02 UAT-07-01):** This assumption is UNVERIFIED on Linux.
//! Before shipping, confirm on a real Windows install that the staged `.nupkg`
//! in `%LocalAppData%\BrevlyPrint\packages\` survives past the 60s timeout when the
//! process stays alive. If it does NOT persist, switch to Design B by:
//!   1. Having `stage_update` call only `download_updates()` then return `Ok(())`.
//!   2. Adding `pub fn apply_staged_on_exit()` that calls `wait_exit_then_apply_updates`.
//!   3. Wiring `apply_staged_on_exit()` from the "Sair" handler in `main.rs`.
//! See `.planning/phases/07-auto-update-distribution-polish/07-UAT.md` UAT-07-01.

use velopack::{sources::HttpSource, UpdateManager};

/// Stage the update package via Velopack SDK (Design A — immediate call).
///
/// Downloads the package from the feed at `feed_base_url` (derived from
/// `downloadUrl` by stripping the filename). Only called after `verify_sha256`
/// passes in `try_check_and_stage` — a mismatch never reaches this function (D-02).
///
/// The `wait_exit_then_apply_updates` call (silent=true, restart=false) spawns the
/// Velopack updater process. The updater waits up to 60s for this process to exit;
/// when the agent exits naturally (reboot / "Sair"), the staged update is applied.
/// On the next launch, `VelopackApp::build().run()` (already the first call in
/// `main()`) picks up the staged package and applies it before any logic runs (SC-3).
///
/// On `Err` from `UpdateManager::new` (e.g., dev build / not a Velopack install —
/// Pitfall 2): returns `Err` with context; the caller logs and retries on the next
/// poll. Never panics.
///
/// # OQ-1 / OQ-2 status
///
/// - **OQ-1 (staging persistence):** UNVERIFIED on Linux — confirm on Windows
///   (see UAT-07-01). If the `.nupkg` is lost after 60s, switch to Design B.
/// - **OQ-2 (`UpdateInfo` field name):** `update.to_apply` is ASSUMED based on
///   RESEARCH.md §Pattern 1 and docs.rs/velopack/1.2.0. Confirm against
///   `UpdateInfo` struct in the 1.2.0 docs — if the field name differs, update this
///   call site accordingly.
pub fn stage_update(feed_base_url: &str) -> anyhow::Result<()> {
    let um = UpdateManager::new(HttpSource::new(feed_base_url), None, None)
        .map_err(|e| anyhow::anyhow!("UpdateManager::new failed (not a Velopack install?): {e}"))?;

    let update = match um
        .check_for_updates()
        .map_err(|e| anyhow::anyhow!("check_for_updates: {e}"))?
    {
        velopack::UpdateCheck::UpdateAvailable(info) => info,
        // SDK says no update — should align with our semver check upstream.
        _ => return Ok(()),
    };

    um.download_updates(&update, None)
        .map_err(|e| anyhow::anyhow!("download_updates: {e}"))?;

    // Design A: call wait_exit_then_apply_updates immediately after download.
    // silent=true  → no Velopack UI (agent is headless SC-1).
    // restart=false → do NOT relaunch; apply strictly on next natural boot (D-05).
    //
    // OQ-1: staging-persistence past the 60s updater timeout is UNVERIFIED on Linux
    // — confirm on Windows (see 07-02 UAT UAT-07-01).
    //
    // OQ-2: `update.to_apply` is the assumed UpdateInfo field (RESEARCH.md A2 / Pattern 1).
    // Confirm against docs.rs/velopack/1.2.0/velopack/struct.UpdateInfo.html before
    // the first Windows release build — if the field name differs, this won't compile.
    um.wait_exit_then_apply_updates(&update.to_apply, true, false, std::iter::empty::<&str>())
        .map_err(|e| anyhow::anyhow!("wait_exit_then_apply_updates: {e}"))?;

    Ok(())
}
