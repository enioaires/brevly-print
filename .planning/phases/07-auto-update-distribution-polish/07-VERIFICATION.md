---
phase: 07-auto-update-distribution-polish
verified: 2026-07-16T18:00:00Z
status: human_needed
score: 8/11 must-haves verified (3 require Windows hardware)
overrides_applied: 0
human_verification:
  - test: "SC-1 — Background download does not interrupt printing and one toast fires"
    expected: "Print job completes in <1s while update downloads; tray icon color unchanged; status line shows 'Atualização pronta — será aplicada ao reiniciar'; exactly one toast fires on the first poll and not again on the 6h re-poll"
    why_human: "Requires a Velopack-installed Windows binary + reachable /api/agent/version + thermal printer. apply.rs is #![cfg(windows)] and excluded from the Linux build."
  - test: "UAT-07-01 — OQ-1 spike: staged .nupkg persistence past 60s updater timeout"
    expected: "After download_updates() + wait_exit_then_apply_updates() are called, the process stays alive >90s; inspect %LocalAppData%\\BrevlyPrint\\packages\\ for the staged .nupkg. If it persists: Design A confirmed (apply.rs correct as-is). If lost: Design B required (split stage/apply; see UAT-07-01). Also confirm update.to_apply is the correct UpdateInfo field name."
    why_human: "Requires a real Velopack install on Windows; the Rust SDK has no TestVelopackLocator; UpdateManager::new() fails on dev builds. Cannot be validated on Linux."
  - test: "UAT-07-02 / SC-2 on Windows — SHA256 mismatch aborts cleanly with a tampered artifact"
    expected: "No tray change, no toast; agent still running as v0.1.0 after relaunch. (The Linux integration test sc2_mismatch_aborts_without_staging proves Ok(false), but the Windows apply path is excluded from that test.)"
    why_human: "The apply.rs code path is #![cfg(windows)] and never compiled or reached on Linux. Requires a Windows install."
  - test: "SC-3 — New version runs after next reboot with no owner action"
    expected: "After staging completes (tray shows 'Atualização pronta'), reboot or relaunch the agent. VelopackApp::build().run() (already first call in main()) applies the staged update; agent reports v0.1.1 in the Sobre dialog."
    why_human: "Requires a Velopack-installed binary + reboot on Windows. The bootstrapper call is present in main.rs (line 357) but its effect is only observable on a real Windows Velopack install."
---

# Phase 07: Auto-Update + Distribution Polish — Verification Report

**Phase Goal:** The agent silently downloads and applies updates on the next reboot without any action from the restaurant owner, with integrity verified before applying.
**Verified:** 2026-07-16T18:00:00Z
**Status:** human_needed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `verify_sha256` returns Ok on matching hash and Err on tampered/mismatched hash | VERIFIED | `src/update/verify.rs` contains pure `pub fn verify_sha256` with `eq_ignore_ascii_case`; 6 unit tests pass (match, mismatch, uppercase, wrong-length, empty-match, empty-mismatch); `cargo test --lib` 29/29 green |
| 2 | `verify_sha256` is case-insensitive and rejects wrong-length/malformed hex | VERIFIED | `computed.eq_ignore_ascii_case(expected_hex)` — confirmed in source (line 19). Wrong-length test passes via `tampered_bytes_returns_err` and `wrong_length_hex_returns_err` tests |
| 3 | `check_for_update` returns UpdateAvailable when remote > current, UpToDate when remote <= current, Err on unparseable semver | VERIFIED | `src/update/check.rs` — 5 unit tests pass covering all three arms; `cargo test --lib` includes them |
| 4 | On a SHA256 mismatch, `try_check_and_stage` returns `Ok(false)` — the Windows stage_update path is never reached and no UpdateStaged signal is produced (SC-2) | VERIFIED | `tests/update_task_test.rs::sc2_mismatch_aborts_without_staging` drives the full orchestration with a wrong sha256; dual assertion `matches!(result, Ok(false))` + `!matches!(result, Ok(true))`; `cargo test --test update_task_test` 6/6 green |
| 5 | `check_version()` performs a bearer-authed GET to `/api/agent/version` and parses `{version, downloadUrl, sha256}` | VERIFIED | `src/noren_client.rs:383-403` — `.bearer_auth(agent_token)`, `VersionResponse` with `#[serde(rename_all = "camelCase")]`; integration test `check_version_200_parses_camel_case` passes; non-200 test passes |
| 6 | When an update is staged, the tray status line shows the correct text and exactly one toast fires; tray icon color does NOT change (D-04) | VERIFIED (code) / UNCERTAIN (runtime) | `src/tray_runtime.rs:72-79` — `set_update_status` sets status text + tooltip, no `set_icon` call confirmed by grep. `show_update_ready_toast()` in `main.rs:295-306`; `update_staged` bool in `run_update_check_loop` gates once-per-session. **Runtime behavior requires Windows hardware.** |
| 7 | The update-check task runs as a fifth Tokio sibling, off the print critical path (SC-1) | VERIFIED (code) | `src/main.rs:613-619` — fifth spawn after retry spawn; clones from `retry_token`/`retry_base_url` BEFORE the retry move (line 590-592). `cargo build` green on Linux. **No-interruption claim requires Windows E2E.** |
| 8 | `vpk pack --channel win --delta BestSpeed` produces the Velopack update package; CI surfaces `{version, downloadUrl, sha256}` for Noren; upload includes `.nupkg` + `releases.win.json` + `assets.win.json`; signing gate unchanged | VERIFIED | `.github/workflows/ci.yml` — `--channel win`, `--delta BestSpeed` present (lines 93-94); `update_info` step writes to `GITHUB_OUTPUT` (lines 115-138); upload path includes `*.nupkg`, `releases.win.json`, `assets.win.json` (lines 144-148); signing gate `CODESIGN_PFX_BASE64 != ''` unchanged (line 102); YAML validates clean |
| 9 | `stage_update` in `apply.rs` uses `UpdateManager`, `check_for_updates`, `download_updates`, `wait_exit_then_apply_updates` with all results `?`-propagated (no `.unwrap()`/`.expect()`) | VERIFIED (code; uncompilable on Linux) | All 4 Velopack calls confirmed in `src/update/apply.rs:67-93`; grep for `unwrap`/`expect` returns zero hits; SPIKE placeholder removed; `update.to_apply` used with OQ-2 caveat comment |
| 10 | On next Windows launch after staging, `VelopackApp::build().run()` applies the update without owner action (SC-3) | UNCERTAIN | `main.rs:357` confirms the first call is `velopack::VelopackApp::build().run()` under `#[cfg(windows)]`, which is the correct bootstrapper position. **Whether staging actually persists and is applied requires Windows UAT (UAT-07-01 + UAT-07-02).** |
| 11 | Background download does not interrupt printing; tray icon color unchanged; one toast fires on update-ready (SC-1) | UNCERTAIN | Code structure correct (fifth task, `update_staged` gate, no `set_icon` in `set_update_status`). **Observable behavior requires Windows hardware + thermal printer + live `/api/agent/version` endpoint.** |

**Score:** 8/11 truths verified on Linux (3 UNCERTAIN pending Windows UAT)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/update/verify.rs` | Pure `verify_sha256(bytes, expected_hex) -> anyhow::Result<()>` | VERIFIED | Exists, substantive (6 unit tests), wired via `try_check_and_stage` and re-exported from `src/update/mod.rs` |
| `src/update/check.rs` | Pure `check_for_update` + `UpdateDecision` enum | VERIFIED | Exists, substantive (5 unit tests), wired in `try_check_and_stage` match |
| `src/update/mod.rs` | `try_check_and_stage` orchestration + re-exports | VERIFIED | Exists, substantive; `verify_sha256` called before `#[cfg(windows)]` staging; Linux no-op stub present |
| `src/update/apply.rs` | Windows-gated `stage_update` with all 4 Velopack calls | VERIFIED (code) / UNCOMPILED (Linux) | File-level `#![cfg(windows)]` confirmed; all 4 SDK calls present; `?`-propagated; OQ-1/OQ-2 caveats documented; not compiled on Linux by design |
| `src/noren_client.rs` | `check_version()` + `VersionResponse` | VERIFIED | Added at lines 370-404; bearer auth confirmed; camelCase serde mapping confirmed |
| `tests/update_task_test.rs` | SC-2 abort + HTTP-mock tests | VERIFIED | 6 tests; `spawn_stub` + `spawn_bytes_stub` helpers; SC-2 dual-assertion present; all 6 pass |
| `src/main.rs` | `UserEvent::UpdateStaged` + `run_update_check_loop` + `show_update_ready_toast` + UpdateStaged handler + fifth spawn | VERIFIED (code) | All present at confirmed line numbers; `update_token = retry_token.clone()` confirmed at line 590; `VelopackApp::build().run()` first at line 357 |
| `src/tray_runtime.rs` | `set_update_status()` — status line + tooltip, no icon change | VERIFIED (code) | Lines 72-79; `set_text` + `set_tooltip` present; grep confirms no `set_icon` in `set_update_status` body |
| `.github/workflows/ci.yml` | Extended Windows job with vpk pack flags + extract step + upload | VERIFIED | YAML valid; `--channel win`, `--delta BestSpeed`, `GITHUB_OUTPUT`, `releases.win.json`, `assets.win.json`, `*.nupkg` all confirmed |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/update/mod.rs::try_check_and_stage` | `src/update/verify.rs::verify_sha256` | mismatch → early return `Ok(false)`, stage skipped | VERIFIED | `verify_sha256` called at line 68 of mod.rs; `#[cfg(windows)]` staging at line 74 — ordering enforced |
| `src/update/mod.rs::try_check_and_stage` | `src/noren_client.rs::check_version` | GET `/api/agent/version` → `VersionResponse` | VERIFIED | `check_version(http, base_url, agent_token)` at line 45 of mod.rs |
| `src/lib.rs` | `src/update/mod.rs` | `pub mod update` | VERIFIED | `pub mod update;` confirmed at `src/lib.rs:26` |
| `src/main.rs::run_update_check_loop` spawn | `src/update/mod.rs::try_check_and_stage` | fifth tokio task, `EventLoopProxy<UserEvent>` | VERIFIED | `run_update_check_loop` at main.rs:316; fifth spawn at lines 613-619; clones from `retry_token`/`retry_base_url` |
| `src/update/mod.rs::try_check_and_stage` (windows) | `src/update/apply.rs::stage_update` | verified bytes → Velopack stage | VERIFIED (code) | `crate::update::apply::stage_update(feed_base_url)?` inside `#[cfg(windows)]` block at mod.rs:83 |
| `src/main.rs::UserEvent::UpdateStaged` | `src/tray_runtime.rs::set_update_status` | event-loop thread mutation | VERIFIED (code) | `user_event()` arm at main.rs:219; calls `rt.set_update_status()` under `#[cfg(windows)]` at line 222-223 |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|--------------|--------|--------------------|--------|
| `src/update/mod.rs` `try_check_and_stage` | `ver_info` (VersionResponse) | `check_version()` → HTTP GET to `/api/agent/version` | Yes — bearer-authed real HTTP; mocked in tests | FLOWING |
| `src/update/mod.rs` `try_check_and_stage` | `bytes` (artifact bytes) | `http.get(&ver_info.download_url)` | Yes — real HTTP GET to download URL | FLOWING |
| `src/tray_runtime.rs` `set_update_status` | status text (static string) | called from `user_event(UpdateStaged)` after real staging | Static string intentionally (D-04 — update-ready is a fixed message) | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Library builds on Linux with apply.rs excluded | `cargo build` | `Finished dev profile` (0 crates compiled) | PASS |
| 29 lib unit tests pass | `cargo test --lib` | 29 passed | PASS |
| 6 integration tests pass (check_version + SC-2 + no-panic) | `cargo test --test update_task_test` | 6 passed | PASS |
| CI YAML is valid | `python3 -c "import yaml; yaml.safe_load(...)"` | `yaml-ok` | PASS |
| CI flags present: `--channel win`, `--delta BestSpeed`, `GITHUB_OUTPUT`, feed files | grep checks | all present | PASS |
| Signing gate unchanged | `grep "CODESIGN_PFX_BASE64 != ''"` | line 102 intact | PASS |
| No forbidden upload commands (vpk upload / aws s3 / wrangler) | grep | 0 matches | PASS |
| No debt markers (TBD/FIXME/XXX) in phase-touched files | grep | 0 matches | PASS |
| Windows E2E: SC-1/SC-2/SC-3 apply path | Requires Windows + reboot | Not run | SKIP (Windows hardware required) |

### Probe Execution

No conventional `scripts/*/tests/probe-*.sh` probes defined for this phase. Spot-checks above serve as the automated verification tier.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| DIST-02 | 07-01, 07-02, 07-03 | Auto-update: agent downloads and installs new version automatically on next reboot, no owner action | PARTIAL — code complete, Windows E2E pending | `try_check_and_stage` → `stage_update` → `VelopackApp::build().run()` pipeline is built; CI produces the update package; end-to-end apply on next reboot requires Windows UAT |
| DIST-03 | 07-01, 07-02 | SHA256 integrity verification before applying any update | VERIFIED | `verify_sha256` (pure, Linux-tested); called before any staging in `try_check_and_stage`; SC-2 integration test proves mismatch aborts with `Ok(false)`; no staging reached |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/update/apply.rs` | 86-89 | `// OQ-1: ... UNVERIFIED on Linux` and `// OQ-2: update.to_apply is ASSUMED` comments | Info | Intentional documentation of open questions pending Windows UAT; these are accurate caveats, not stubs — the function body is complete code, not a placeholder |

No `TBD`, `FIXME`, or `XXX` markers found in any phase-touched file. The OQ-1/OQ-2 comments in `apply.rs` are informational engineering notes referencing UAT-07-01, not unresolved debt.

### Human Verification Required

#### 1. UAT-07-01: OQ-1 Spike — Velopack Staging Persistence Past 60s Updater Timeout

**Test:** On a Windows machine with a Velopack-installed `BrevlyPrint` v0.1.0:
1. Publish a local Velopack feed for v0.1.1 (`releases.win.json` + `.nupkg`) reachable by `HttpSource`.
2. Trigger the update-check loop (10s startup delay, then `try_check_and_stage` calls `stage_update`).
3. After `download_updates()` + `wait_exit_then_apply_updates(&update.to_apply, true, false, [])` are called, keep the process alive for >90 seconds.
4. Inspect `%LocalAppData%\BrevlyPrint\packages\` — is the staged `.nupkg` still present?
5. Exit and relaunch — does the agent come up as v0.1.1?

**Expected (Design A confirmed):** Staged `.nupkg` survives the 60s timeout; agent v0.1.1 on relaunch. If not (Design B): update `apply.rs` to call only `download_updates` in `stage_update`, add `apply_staged_on_exit()`, wire it to the "Sair" handler.

**Also confirm:** `update.to_apply` is the correct `UpdateInfo` field name on `velopack 1.2.0`. If not, update `apply.rs` line 92.

**Why human:** The Rust Velopack SDK has no `TestVelopackLocator`; `UpdateManager::new()` errors on non-installed dev builds; `apply.rs` is `#![cfg(windows)]` and excluded from Linux compilation.

#### 2. UAT-07-02 / SC-1: Background Download Does Not Interrupt Printing

**Test:** Point the agent at a `/api/agent/version` serving `{"version":"0.1.1","downloadUrl":"<url>","sha256":"<correct>"}`. While the (throttled) download runs, trigger a print event from Noren.

**Expected:** Comanda prints in <1s; tray icon color unchanged (stays health-state color); status line shows `"Atualização pronta — será aplicada ao reiniciar"`; exactly one toast fires; no second toast on the 6h re-poll.

**Why human:** Requires a Windows machine with a Velopack install, thermal printer, and reachable `/api/agent/version` endpoint.

#### 3. UAT-07-02 / SC-2 on Windows: SHA256 Mismatch Aborts on Real Windows Apply Path

**Test:** Restart the agent; serve a `sha256` that does NOT match the hosted `.nupkg` (e.g., all zeros). Observe tray and relaunch.

**Expected:** No tray change, no toast, agent still v0.1.0 after relaunch. (The Linux `sc2_mismatch_aborts_without_staging` test proves `Ok(false)`, but the Windows `stage_update` path is excluded from that test path.)

**Why human:** `apply.rs` is `#![cfg(windows)]`; the `Ok(false)` path is Linux-proven but the cfg-excluded staging code must also be confirmed absent on mismatch via a real Windows run.

#### 4. UAT-07-02 / SC-3: New Version Runs After Reboot With No Owner Action

**Test:** With correct sha256, let the update stage (status line shows "Atualização pronta"). Reboot or relaunch the agent.

**Expected:** Agent comes up as v0.1.1 (Sobre dialog shows 0.1.1) with zero manual action — `VelopackApp::build().run()` applied the staged update.

**Why human:** Requires a Velopack-installed binary and a reboot on Windows.

### Gaps Summary

No blocking gaps identified. The Linux-provable vertical slice (DIST-03 integrity gate, SC-2 mismatch abort, check_version bearer auth, CI update package, binary compilation, 35 tests green) is complete and verified. The three human-needed items are all Windows-hardware gates that cannot be observed programmatically in a Linux dev environment and are accurately captured in `.planning/phases/07-auto-update-distribution-polish/07-UAT.md` as UAT-07-01 and UAT-07-02.

DIST-02 is code-complete pending Windows E2E confirmation. DIST-03 is fully verified on Linux.

---

_Verified: 2026-07-16T18:00:00Z_
_Verifier: Claude (gsd-verifier)_
