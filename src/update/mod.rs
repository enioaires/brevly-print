//! Auto-update pipeline — Linux-provable core.
//!
//! This module owns the pure update logic that compiles and tests on Linux:
//!   - `check_for_update` (semver comparison)
//!   - `verify_sha256` (SHA-256 integrity gate)
//!   - `try_check_and_stage` (orchestration seam)
//!
//! What is NOT here (plan 02):
//!   - `run_update_check_loop` — needs `crate::UserEvent` which lives in the
//!     binary crate (`main.rs`), not the library. Moving it here would require
//!     either making `UserEvent` generic or re-exporting it from lib, both of
//!     which add complexity. Keeping the loop in `main.rs` is the minimal-diff
//!     boundary: `main.rs` imports `try_check_and_stage` and drives the loop.

pub mod check;
pub mod verify;

#[cfg(windows)]
pub mod apply;

// Re-export public surface for callers (e.g., main.rs and integration tests):
pub use check::{check_for_update, UpdateDecision};
pub use verify::verify_sha256;

use anyhow::Context as _;
use crate::noren_client::check_version;

/// One check-and-stage cycle.
///
/// Returns:
/// - `Ok(true)`  — update downloaded, SHA256 verified, staged (Windows) or stubbed (Linux).
/// - `Ok(false)` — up to date, version parse error, or SHA256 mismatch (SC-2 abort).
/// - `Err(_)`    — HTTP transport or response parse failure; caller should log and retry.
///
/// Security:
/// - `agent_token` is passed only to `check_version` which uses `.bearer_auth()` — never
///   interpolated into any log or error string (T-02-02).
/// - `verify_sha256` is called BEFORE any `#[cfg(windows)]` staging call — a mismatch
///   aborts without touching the running agent (D-02 / SC-2).
pub async fn try_check_and_stage(
    http: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
) -> anyhow::Result<bool> {
    let ver_info = check_version(http, base_url, agent_token)
        .await
        .context("try_check_and_stage: version check failed")?;

    let current = env!("CARGO_PKG_VERSION");
    match check_for_update(current, &ver_info.version) {
        UpdateDecision::UpToDate => return Ok(false),
        UpdateDecision::Err(e) => anyhow::bail!("try_check_and_stage: version parse: {e}"),
        UpdateDecision::UpdateAvailable => {} // continue to download + verify
    }

    // Download artifact for manual SHA256 integrity check (D-02 belt-and-suspenders).
    let bytes = http
        .get(&ver_info.download_url)
        .send()
        .await
        .context("try_check_and_stage: artifact download transport error")?
        .bytes()
        .await
        .context("try_check_and_stage: artifact download body error")?;

    // SC-2: mismatch → abort; no staging, no UpdateStaged signal, running agent untouched.
    // eprintln! is the only output — no owner-facing error (D-02).
    if let Err(e) = verify_sha256(&bytes, &ver_info.sha256) {
        eprintln!("[brevly-print] Update aborted — SHA256 mismatch: {e:#}");
        return Ok(false);
    }

    // Stage via Velopack SDK (Windows-only). Linux: no-op log stub.
    #[cfg(windows)]
    {
        // Derive feed base URL from downloadUrl (strip filename, keep directory path).
        // The Velopack feed files (releases.win.json + .nupkg) must be at the same path.
        let feed_base_url = ver_info
            .download_url
            .rsplit_once('/')
            .map(|(base, _)| base)
            .unwrap_or(&ver_info.download_url);
        crate::update::apply::stage_update(feed_base_url)?;
    }
    #[cfg(not(windows))]
    eprintln!("[brevly-print] Update staged (Linux stub — Velopack apply is Windows-only)");

    Ok(true)
}
