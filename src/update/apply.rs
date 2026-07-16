#![cfg(windows)]
//! Windows-only: Velopack UpdateManager staging.
//!
//! Compiled only when `cfg(windows)`. On Linux, `src/update/mod.rs` provides
//! a no-op stub so the surrounding logic remains testable (D-07).
//!
//! This is a skeleton finalized in plan 02 after the OQ-1 spike confirms:
//!   - Whether `download_updates()` persists the .nupkg independently of process lifetime
//!   - The exact `UpdateInfo` field name (`to_apply`? or another field?)
//!   - Whether `wait_exit_then_apply_updates` should be called here or deferred to Sair handler

use velopack::{sources::HttpSource, UpdateManager};

/// Stage the update package via Velopack SDK.
///
/// Downloads the package from the feed at `feed_base_url` (derived from
/// `downloadUrl` by stripping the filename). Verifies via our manual SHA256
/// before this call — this function is only reached on SHA256 match (D-02).
///
/// NOTE on `wait_exit_then_apply_updates` timing (Pitfall 1 / OQ-1):
/// Do NOT call this immediately from the background task if the 60s updater
/// timeout causes stage loss. Planner must spike-validate whether `download_updates()`
/// persists the .nupkg independently; if so, call `wait_exit_then_apply_updates`
/// only at process exit (Sair handler or App Drop impl). See RESEARCH.md OQ1/OQ2.
pub fn stage_update(feed_base_url: &str) -> anyhow::Result<()> {
    let um = UpdateManager::new(HttpSource::new(feed_base_url), None, None)
        .map_err(|e| anyhow::anyhow!("UpdateManager::new failed (not a Velopack install?): {e}"))?;

    let update = match um
        .check_for_updates()
        .map_err(|e| anyhow::anyhow!("check_for_updates: {e}"))?
    {
        velopack::UpdateCheck::UpdateAvailable(info) => info,
        // SDK says no update — should align with our semver check upstream
        _ => return Ok(()),
    };

    um.download_updates(&update, None)
        .map_err(|e| anyhow::anyhow!("download_updates: {e}"))?;

    // With silent=true, restart=false: updater process waits for this process to exit,
    // then applies. Agent keeps printing (SC-1). New version appears on next natural boot.
    // SPIKE REQUIRED (OQ-1, plan 02): confirm the staged .nupkg persists if the 60s
    // timeout elapses before the agent exits. Also confirm `update.to_apply` is the
    // correct field name on UpdateInfo.
    um.wait_exit_then_apply_updates(&update.to_apply, true, false, std::iter::empty::<&str>())
        .map_err(|e| anyhow::anyhow!("wait_exit_then_apply_updates: {e}"))?;

    Ok(())
}
