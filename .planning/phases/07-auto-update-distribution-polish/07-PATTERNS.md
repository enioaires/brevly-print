# Phase 7: Auto-Update + Distribution Polish — Pattern Map

**Mapped:** 2026-07-16
**Files analyzed:** 9 new/modified files
**Analogs found:** 9 / 9

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/update/mod.rs` | module-root | batch (loop) | `src/retry_task.rs` (public fn re-export pattern) | role-match |
| `src/update/check.rs` | utility (pure logic) | transform | `src/health_state.rs` (pure enum + pure fns, Linux-testable) | exact |
| `src/update/verify.rs` | utility (pure logic) | transform | `src/health_state.rs` (pure, no Windows dep) | exact |
| `src/update/apply.rs` | service (Windows-gated) | request-response | `src/tray_runtime.rs` (file-level `#![cfg(windows)]`) | exact |
| `src/noren_client.rs` | service (HTTP client) | request-response | `src/noren_client.rs` itself — `fetch_pending_jobs()` + `fetch_job_bytes()` | self-analog |
| `src/main.rs` | entry-point | event-driven | `src/main.rs` existing spawn block + `UserEvent` enum | self-analog |
| `src/tray_runtime.rs` | service (Windows tray) | event-driven | `src/tray_runtime.rs` `apply_health()` method | self-analog |
| `.github/workflows/ci.yml` | config (CI) | batch | `.github/workflows/ci.yml` existing Windows job | self-analog |
| `tests/update_task_test.rs` | test | request-response | `tests/noren_client_test.rs` + `tests/retry_task_test.rs` | exact |

---

## Pattern Assignments

### `src/update/check.rs` (utility, pure transform)

**Analog:** `src/health_state.rs` (pure enum state machine, no Windows dep, Linux-testable)

**Module header + imports pattern** — mirrors how `health_state.rs` uses only portable crates:

```rust
// src/update/check.rs — portable, no #[cfg(windows)] anywhere in this file.
// Must compile and pass `cargo test --lib` on Linux (D-07).

use semver::Version;
// semver is already in Cargo.lock (transitive via velopack = "1"). No Cargo.toml entry needed
// unless explicit pinning is desired (add `semver = "1.0.28"` to [dependencies] if so).
```

**Core pattern — pure enum + pure fn:**

```rust
pub enum UpdateDecision {
    UpToDate,
    UpdateAvailable,
    Err(String),
}

/// Pure: no I/O, no Windows dep. Linux unit-testable (D-07).
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

**In-module unit tests** — copy the `#[cfg(test)]` block structure from `src/tray_runtime.rs` lines 121–142 and `src/retry_task.rs` line 508+:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_remote_returns_available() {
        assert!(matches!(
            check_for_update("0.1.0", "0.2.0"),
            UpdateDecision::UpdateAvailable
        ));
    }

    #[test]
    fn older_remote_returns_up_to_date() {
        assert!(matches!(
            check_for_update("0.2.0", "0.1.0"),
            UpdateDecision::UpToDate
        ));
    }

    #[test]
    fn equal_versions_returns_up_to_date() {
        assert!(matches!(
            check_for_update("0.1.0", "0.1.0"),
            UpdateDecision::UpToDate
        ));
    }

    #[test]
    fn malformed_remote_returns_err() {
        assert!(matches!(
            check_for_update("0.1.0", "not-semver"),
            UpdateDecision::Err(_)
        ));
    }
}
```

---

### `src/update/verify.rs` (utility, pure transform)

**Analog:** `src/health_state.rs` (pure module, no Windows dep)

**Imports pattern:**

```rust
// src/update/verify.rs — portable. No #[cfg(windows)] anywhere.
// sha2 0.11.0 is already in Cargo.lock (transitive via velopack = "1").
// Do NOT add sha2 = "0.10" — that creates a second incompatible major version (Pitfall 4).
// Explicit dep to add (optional but recommended to make intent clear):
//   sha2 = "0.11" in [dependencies] of Cargo.toml
use sha2::{Sha256, Digest};
```

**Core pure function:**

```rust
/// Verify that `bytes` SHA-256 hashes to `expected_hex` (case-insensitive hex).
///
/// Returns `Ok(())` on match.
/// Returns `Err` on mismatch, empty input producing wrong length, or malformed hex.
/// Pure function — no I/O, no Windows deps — Linux unit-testable (D-02 / D-07).
pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> anyhow::Result<()> {
    let hash = Sha256::digest(bytes);
    // Hex-encode without an extra crate (`hex` is not in Cargo.lock):
    let computed: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    if computed.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        anyhow::bail!(
            "SHA256 mismatch: expected {expected_hex}, got {computed}"
        )
    }
}
```

**In-module unit tests** (same `#[cfg(test)]` pattern as rest of codebase):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Sha256, Digest};
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn correct_bytes_and_hex_ok() {
        let data = b"hello world";
        let hex = sha256_hex(data);
        assert!(verify_sha256(data, &hex).is_ok());
    }

    #[test]
    fn tampered_bytes_returns_err() {
        let data = b"hello world";
        let hex = sha256_hex(data);
        let mut tampered = data.to_vec();
        tampered[0] ^= 0xFF;
        assert!(verify_sha256(&tampered, &hex).is_err());
    }

    #[test]
    fn uppercase_expected_hex_accepted() {
        let data = b"hello world";
        let hex = sha256_hex(data).to_uppercase();
        assert!(verify_sha256(data, &hex).is_ok());
    }

    #[test]
    fn wrong_length_hex_returns_err() {
        assert!(verify_sha256(b"data", "abc").is_err()); // 3 hex chars, not 64
    }
}
```

---

### `src/update/apply.rs` (service, Windows-gated, request-response)

**Analog:** `src/tray_runtime.rs` — the entire file uses `#![cfg(windows)]` at the top (line 1), meaning it compiles to nothing on Linux. `apply.rs` uses the identical pattern.

**File-level cfg gate** (exact pattern from `src/tray_runtime.rs` line 1):

```rust
#![cfg(windows)]
//! Windows-only: Velopack UpdateManager staging.
//!
//! Compiled only when `cfg(windows)`. On Linux, `src/update/mod.rs` provides
//! a no-op stub so the surrounding logic remains testable (D-07).
```

**Imports pattern:**

```rust
use velopack::{UpdateManager, sources::HttpSource};
```

**Core stage function:**

```rust
/// Stage the update package via Velopack SDK.
///
/// Downloads the package from the feed at `feed_base_url` (derived from
/// `downloadUrl` by stripping the filename). Verifies via our manual SHA256
/// before this call — this function is only reached on SHA256 match (D-02).
///
/// NOTE on `wait_exit_then_apply_updates` timing (Pitfall 1 / OQ1):
/// Do NOT call this immediately from the background task if the 60s updater
/// timeout causes stage loss. Planner must spike-validate whether `download_updates()`
/// persists the .nupkg independently; if so, call `wait_exit_then_apply_updates`
/// only at process exit (Sair handler or App Drop impl). See RESEARCH.md OQ1/OQ2.
pub fn stage_update(feed_base_url: &str) -> anyhow::Result<()> {
    let um = UpdateManager::new(HttpSource::new(feed_base_url), None, None)
        .map_err(|e| anyhow::anyhow!("UpdateManager::new failed (not a Velopack install?): {e}"))?;

    let update = match um.check_for_updates()
        .map_err(|e| anyhow::anyhow!("check_for_updates: {e}"))? {
        velopack::UpdateCheck::UpdateAvailable(info) => info,
        _ => return Ok(()), // SDK says no update (should align with our version check)
    };

    um.download_updates(&update, None)
        .map_err(|e| anyhow::anyhow!("download_updates: {e}"))?;

    // With silent=true, restart=false: updater process waits for this process to exit,
    // then applies. Agent keeps printing (SC-1). New version appears on next natural boot.
    // SPIKE REQUIRED: confirm the staged .nupkg persists if the 60s timeout elapses.
    um.wait_exit_then_apply_updates(&update.to_apply, true, false, std::iter::empty::<&str>())
        .map_err(|e| anyhow::anyhow!("wait_exit_then_apply_updates: {e}"))?;

    Ok(())
}
```

**ASSUMED:** `update.to_apply` is the field name on `UpdateInfo`. Planner must verify against
`docs.rs/velopack/1.2.0` — the actual type/field may differ (see RESEARCH.md Assumption A2).

---

### `src/update/mod.rs` (module-root, background task loop)

**Analog:** `src/retry_task.rs` — public async loop function, cloned resource pattern, `eprintln!` on failure, never panic.

**Module declaration + re-exports:**

```rust
// src/update/mod.rs
pub mod check;
pub mod verify;

#[cfg(windows)]
pub mod apply;

// Re-export public surface for callers (e.g., main.rs imports check_for_update, verify_sha256):
pub use check::{check_for_update, UpdateDecision};
pub use verify::verify_sha256;
```

**Background loop function** — mirrors the Pusher task + retry task spawn pattern from
`src/main.rs` lines 509–536:

```rust
use crate::noren_client::check_version;
use crate::update::{check::UpdateDecision, check::check_for_update, verify::verify_sha256};

/// Run the update-check loop (D-03).
///
/// - Startup delay: ~10s after tray is visible and Pusher connect is underway.
/// - Polls every ~6h.
/// - On failure: logs to stderr, continues. Never panics, never blocks printing (SC-1).
/// - On staged update: sends `UserEvent::UpdateStaged` once via proxy (D-04).
pub async fn run_update_check_loop(
    http: reqwest::Client,
    base_url: String,
    agent_token: String,
    proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>,
) {
    // D-03: startup delay — tray must be visible, Pusher connect underway.
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    let mut update_staged = false; // gate: toast fires once per session (D-04 anti-double-toast)

    loop {
        if !update_staged {
            match try_check_and_stage(&http, &base_url, &agent_token).await {
                Ok(true) => {
                    let _ = proxy.send_event(crate::UserEvent::UpdateStaged);
                    update_staged = true;
                }
                Ok(false) => {} // up to date or mismatch — silent (D-03 / CONTEXT specifics)
                Err(e) => eprintln!("[brevly-print] Update check failed: {e:#}"),
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(6 * 60 * 60)).await;
    }
}

/// Inner: one check+stage cycle. Returns Ok(true) on staged, Ok(false) on up-to-date or
/// SHA256 mismatch abort, Err on transport/parse failure.
async fn try_check_and_stage(
    http: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
) -> anyhow::Result<bool> {
    let ver_info = check_version(http, base_url, agent_token).await?;

    let current = env!("CARGO_PKG_VERSION");
    match check_for_update(current, &ver_info.version) {
        UpdateDecision::UpToDate => return Ok(false),
        UpdateDecision::Err(e) => anyhow::bail!("version parse: {e}"),
        UpdateDecision::UpdateAvailable => {}
    }

    // Download artifact for manual SHA256 (D-02 belt-and-suspenders).
    let bytes = http.get(&ver_info.download_url).send().await?.bytes().await?;

    // Mismatch → abort; no staging, no toast, no owner-facing error (D-02).
    if let Err(e) = verify_sha256(&bytes, &ver_info.sha256) {
        eprintln!("[brevly-print] Update aborted — SHA256 mismatch: {e:#}");
        return Ok(false);
    }

    // Stage via Velopack SDK (Windows-only). Linux: no-op log stub.
    #[cfg(windows)]
    {
        // Derive feed base URL from downloadUrl (strip filename, keep directory).
        // The Velopack feed files (releases.win.json + .nupkg) must be at the same path.
        let feed_base_url = ver_info.download_url
            .rsplit_once('/')
            .map(|(base, _)| base)
            .unwrap_or(&ver_info.download_url);
        crate::update::apply::stage_update(feed_base_url)?;
    }
    #[cfg(not(windows))]
    eprintln!("[brevly-print] Update staged (Linux stub — Velopack apply is Windows-only)");

    Ok(true)
}
```

---

### `src/noren_client.rs` — add `check_version()` (service, request-response)

**Analog:** `fetch_pending_jobs()` in same file, lines 333–364. `check_version()` is a near-exact clone.

**New response type** — mirrors `PendingJob` / `ActivateResponse` shape (lines 53–61 and 313–318):

```rust
/// Response from `GET /api/agent/version`.
///
/// Noren returns camelCase JSON; `rename_all` maps to Rust snake_case (Pitfall 7 pattern).
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VersionResponse {
    pub version: String,
    pub download_url: String,
    pub sha256: String,
}
```

**New function** — copy shape from `fetch_pending_jobs()` (lines 333–364) and `fetch_job_bytes()` (lines 266–303):

```rust
/// Fetch the current release version info from `GET {base_url}/api/agent/version`.
///
/// `agent_token` is passed ONLY via `.bearer_auth()` — never logged (T-02-02).
/// On failure: returns `Err`; caller logs and silently retries on next poll (D-03).
pub async fn check_version(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
) -> anyhow::Result<VersionResponse> {
    let url = format!("{base_url}/api/agent/version");

    let resp = client
        .get(&url)
        .bearer_auth(agent_token) // T-02-02: token passed here, never in eprintln!
        .send()
        .await
        .context("check_version: HTTP transport error")?;

    match resp.status().as_u16() {
        200 => resp
            .json::<VersionResponse>()
            .await
            .context("check_version: response parse error"),
        status => anyhow::bail!("check_version: unexpected status {status}"),
    }
}
```

**Key pattern points from analog (lines 286–290, 346–350):**
- `.bearer_auth(agent_token)` — never interpolated into a format string
- `context("check_version: ...")` — function-name prefix on all context strings
- `anyhow::bail!("...: unexpected status {status}")` — non-2xx catch-all
- Local `#[derive(Deserialize)]` struct defined inline (not polluting module namespace)

---

### `src/main.rs` — extend `UserEvent` + spawn update task (entry-point, event-driven)

**Analog:** existing `UserEvent` enum (lines 65–71) and spawn block (lines 506–541).

**Extend `UserEvent` enum** — add one variant alongside `HealthChanged` (line 70):

```rust
// src/main.rs lines 65–71 — existing enum, extend here:
#[derive(Debug)]
enum UserEvent {
    #[cfg(windows)]
    TrayIconEvent(tray_icon::TrayIconEvent),
    #[cfg(windows)]
    MenuEvent(tray_icon::menu::MenuEvent),
    HealthChanged(HealthState),
    UpdateStaged,    // NEW — Phase 7 (D-04): background task signals update ready
}
```

**Handle `UpdateStaged` in `user_event()`** — copy the `HealthChanged` handler pattern (lines 207–215):

```rust
// In App::user_event() match arm — mirrors HealthChanged handler at lines 207–215:
UserEvent::UpdateStaged => {
    #[cfg(windows)]
    if let Some(rt) = &self.tray_runtime {
        rt.set_update_status();   // updates disabled status-line text (D-04)
    }
    // One-shot toast — fires via the same show_*_toast() pattern as retry_task.rs:492–504.
    // The once-per-session gate lives in run_update_check_loop (update_staged flag).
    show_update_ready_toast();
    let _ = event_loop; // suppress unused-variable warning on Linux
}
```

**Import addition** to the `use brevly_print::{ ... }` block (line 26–37 pattern):

```rust
// Add to existing brevly_print imports:
use brevly_print::update::run_update_check_loop;
```

**Spawn update task** — insert inside `if is_runtime { ... }` block AFTER the retry task spawn (lines 523–537), following the exact clone+spawn shape used for pusher (lines 508–511) and retry (lines 523–536):

```rust
// Phase 7: spawn update-check loop — fifth Tokio task.
// Clone values before they are moved into any prior spawn closure.
let proxy_for_update = event_loop.create_proxy();
let update_token     = retry_token.clone();  // agent_token already cloned for retry above
let update_base_url  = retry_base_url.clone();
let update_http      = http.clone();

rt_handle.spawn(async move {
    run_update_check_loop(update_http, update_base_url, update_token, proxy_for_update).await;
});
```

**`show_update_ready_toast()` free function** — add to `main.rs` or a dedicated location, copy from `src/retry_task.rs` lines 488–504:

```rust
// Same pattern as show_print_failure_toast() in retry_task.rs:492–504.
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

---

### `src/tray_runtime.rs` — add `set_update_status()` (service, event-driven)

**Analog:** `apply_health()` method in same file, lines 59–63.

**New method on `TrayRuntime`** — mirrors `apply_health()` exactly but does NOT change the icon color (D-04 decision: icon color reserved for connection health):

```rust
// In impl TrayRuntime — add after apply_health() (line 63):

/// Update the status-line text to the "update ready" message (D-04).
///
/// Does NOT change the tray icon color — icon color is reserved for connection health
/// (Phase 3 D-01/D-02, enforced by D-04). Only the disabled status-line text changes.
pub fn set_update_status(&self) {
    self.menu_items.status.set_text(
        "Atualização pronta — será aplicada ao reiniciar"
    );
    let _ = self.tray.set_tooltip(Some(
        "Brevly Print — Atualização pronta"
    ));
}
```

**The exact analog** (lines 59–63 of `src/tray_runtime.rs`):

```rust
// ANALOG — apply_health() — copy this shape:
pub fn apply_health(&self, health: HealthState) {
    let _ = self.tray.set_icon(Some(health.icon()));   // ← NOT in set_update_status (D-04)
    let _ = self.tray.set_tooltip(Some(health.tooltip()));
    self.menu_items.status.set_text(health.status_label());
}
```

---

### `.github/workflows/ci.yml` — extend Windows job (config, batch)

**Analog:** existing `build-windows` job in `.github/workflows/ci.yml` lines 48–115.

**The `vpk pack` step to extend** (lines 82–93 — already present):

```yaml
# Existing step — DO NOT REMOVE OR CHANGE existing flags. Add --channel and --delta flags:
- name: Package with vpk (produces Setup.exe + update package)
  shell: pwsh
  run: |
    $version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
    vpk pack `
      --packId BrevlyPrint `
      --packVersion $version `
      --packDir target\x86_64-pc-windows-msvc\release `
      --mainExe brevly-print.exe `
      --outputDir Releases `
      --packTitle "Brevly Print" `
      --packAuthors "Brevly" `
      --channel win `
      --delta BestSpeed
    # ^ --channel win produces releases.win.json (required by HttpSource)
    # ^ --delta BestSpeed produces delta .nupkg when previous release is in --outputDir
    # ^ First release: no previous .nupkg in Releases/ → no delta generated (Pitfall 7)
```

**New step — extract update artifact info for Noren** (add AFTER vpk pack, BEFORE upload):

```yaml
- name: Extract update artifact info for Noren /api/agent/version endpoint
  id: update_info
  shell: pwsh
  run: |
    # vpk produces assets.win.json listing artifacts + SHA256 per artifact.
    # [ASSUMED A5]: assets.win.json contains a sha256 field. If absent, compute manually:
    #   $sha256 = (Get-FileHash -Algorithm SHA256 $fullPkgPath).Hash.ToLower()
    $assets = Get-Content Releases/assets.win.json | ConvertFrom-Json
    $fullPkg = $assets.assets | Where-Object { $_.type -eq "Full" } | Select-Object -First 1
    $version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
    echo "version=$version" >> $env:GITHUB_OUTPUT
    echo "filename=$($fullPkg.fileName)" >> $env:GITHUB_OUTPUT
    echo "sha256=$($fullPkg.sha256)" >> $env:GITHUB_OUTPUT
    echo "--- Noren /api/agent/version payload ---"
    echo "{ version: $version, downloadUrl: '<S3_BASE>/$($fullPkg.fileName)', sha256: $($fullPkg.sha256) }"
```

**Upload step — extend to include full update package** (lines 110–115 — extend `path`):

```yaml
# Extend existing upload step to also include the .nupkg + releases.win.json:
- name: Upload release artifacts
  uses: actions/upload-artifact@v4
  with:
    name: brevly-print-release
    path: |
      Releases/*Setup.exe
      Releases/*.nupkg
      Releases/releases.win.json
      Releases/assets.win.json
```

**Signing step** (lines 96–109) — copy-unchanged; it already gates on `CODESIGN_PFX_BASE64` per
D-06 / Phase 3 D-12. The `.nupkg` should also be signed when the cert is present; add the `.nupkg`
to the signing loop if the Noren team wants it. CONTEXT D-06 says signing stays gated as-is.

---

### `tests/update_task_test.rs` (test, request-response)

**Analog 1:** `tests/noren_client_test.rs` — `spawn_stub()` pattern (lines 21–49) for HTTP mock.
**Analog 2:** `tests/retry_task_test.rs` — `#[tokio::test]` + pure-function + mock-HTTP shape.

**File header + mock stub helper** — copy `spawn_stub` from `tests/noren_client_test.rs` lines 21–49:

```rust
//! Tests for Phase 7 update logic.
//!
//! Portable — runs on Linux via `cargo test --lib` (D-07).
//! Pure-function tests (check_for_update, verify_sha256) need no mock.
//! HTTP tests for check_version use the same spawn_stub pattern as noren_client_test.rs.

use brevly_print::update::check::{check_for_update, UpdateDecision};
use brevly_print::update::verify::verify_sha256;
use brevly_print::noren_client::check_version;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

/// Spawn a local HTTP stub (copied from tests/noren_client_test.rs lines 21–49).
async fn spawn_stub(status: u16, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut buf = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
        socket.write_all(response.as_bytes()).await.expect("write");
        socket.shutdown().await.ok();
    });
    format!("http://127.0.0.1:{port}")
}
```

---

## Shared Patterns

### `#[cfg(windows)]` / `#![cfg(windows)]` Gating (D-07 — whole-project pattern)

**Source:** `src/tray_runtime.rs` line 1 (file-level gate) + `src/retry_task.rs` lines 493–503 (inline gate).

**File-level gate** — use for modules that are entirely Windows-only (copy `tray_runtime.rs` line 1):

```rust
#![cfg(windows)]
```

**Inline gate** — use for individual functions/blocks in otherwise portable files:

```rust
#[cfg(windows)]
{
    // Windows-only code here
}
#[cfg(not(windows))]
eprintln!("[brevly-print] <operation> (Linux: stderr only)");
```

**Apply to:** `src/update/apply.rs` (file-level), inline gates in `src/update/mod.rs`
`try_check_and_stage()`, `src/main.rs` `user_event()` UpdateStaged arm, `show_update_ready_toast()`.

---

### Bearer Auth HTTP GET (T-02-02 — never log token)

**Source:** `src/noren_client.rs` `fetch_pending_jobs()` lines 346–350 and `fetch_job_bytes()` lines 286–290.

**Pattern:**

```rust
let resp = client
    .get(&url)
    .bearer_auth(agent_token) // T-02-02: token passed here, never in eprintln!/format!
    .send()
    .await
    .context("function_name: HTTP transport error")?;
```

**Apply to:** `check_version()` in `src/noren_client.rs`.

---

### Background Task Spawn (EventLoopProxy + clone pattern)

**Source:** `src/main.rs` lines 455–459 (retry proxy) and 506–536 (all spawn calls).

**Pattern:**

```rust
let proxy_for_X = event_loop.create_proxy();
let x_token     = agent_token.clone();
let x_base_url  = worker_base_url.clone();
let x_http      = http.clone();

rt_handle.spawn(async move {
    run_X_loop(x_http, x_base_url, x_token, proxy_for_X).await;
});
```

**Apply to:** update task spawn in `src/main.rs`. Clone values in the same order as existing tasks —
all clones happen BEFORE any prior value is moved into a spawn closure.

---

### Toast Notification (fire-and-forget, cfg-gated)

**Source:** `src/retry_task.rs` lines 488–504 — `show_print_failure_toast()`.

**Pattern:**

```rust
fn show_update_ready_toast() {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;
        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title("Brevly Print")
            .text1("...")
            .show();
    }
    #[cfg(not(windows))]
    eprintln!("[brevly-print] ... (Linux: stderr only)");
}
```

**Apply to:** `show_update_ready_toast()` (new function in `src/main.rs` or `src/update/mod.rs`).

---

### `anyhow::bail!` + `context()` Error Handling

**Source:** `src/noren_client.rs` throughout — every async fn uses `.context("fn_name: ...")` on `?` and `anyhow::bail!("fn_name: ...")` for status errors.

**Pattern:**

```rust
.context("check_version: HTTP transport error")?;
// ...
anyhow::bail!("check_version: unexpected status {status}");
```

**Apply to:** all functions in `src/update/mod.rs` and the new `check_version()`.

---

### `env!("CARGO_PKG_VERSION")` — D-01 version comparison base

**Source:** `src/tray_runtime.rs` line 80 — `show_about_dialog()` already reads it.

```rust
let version = env!("CARGO_PKG_VERSION"); // evaluates at compile time
```

**Apply to:** `try_check_and_stage()` in `src/update/mod.rs` — `let current = env!("CARGO_PKG_VERSION");`

---

## No Analog Found

All Phase 7 files have close analogs in the codebase. The Velopack `UpdateManager` API in
`src/update/apply.rs` has no existing usage in the project (only `VelopackApp::build().run()` is
wired in `main.rs`), so the executor must follow RESEARCH.md Pattern 1 and spike-validate the
exact `UpdateInfo` field names and `wait_exit_then_apply_updates` staging-persistence behavior
(RESEARCH.md OQ1/OQ2/A2) before finalizing `apply.rs`.

---

## Metadata

**Analog search scope:** `src/`, `tests/`, `.github/workflows/`
**Files scanned:** 12 (main.rs, noren_client.rs, tray_runtime.rs, retry_task.rs, lib.rs,
Cargo.toml, ci.yml, health_state.rs, noren_client_test.rs, retry_task_test.rs,
pending_jobs_test.rs, print_worker_test.rs)
**Pattern extraction date:** 2026-07-16
