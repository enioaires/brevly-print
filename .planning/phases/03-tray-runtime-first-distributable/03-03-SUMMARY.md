---
phase: 03-tray-runtime-first-distributable
plan: "03"
subsystem: ci-distributable
tags: [ci, packaging, velopack, vpk, signtool, authenticode, windows, dist-01]
dependency_graph:
  requires: [03-01]
  provides: [DIST-01-ci-half]
  affects: [.github/workflows/ci.yml, .planning/STATE.md]
tech_stack:
  added: []
  patterns:
    - vpk CLI packaging step in GitHub Actions Windows job
    - conditional signtool signing gated on GitHub secret presence
    - actions/upload-artifact@v4 for Setup.exe distribution
key_files:
  created: []
  modified:
    - .github/workflows/ci.yml
    - .planning/STATE.md
decisions:
  - vpk pack uses --packId BrevlyPrint --mainExe brevly-print.exe --packTitle "Brevly Print" --packAuthors "Brevly"
  - signtool step gated on secrets.CODESIGN_PFX_BASE64 so CI passes pre-cert (D-12/D-13)
  - PFX decoded to RUNNER_TEMP and deleted immediately after signing (T-03-03-01)
  - STATE.md OV cert todo expanded with exact secret names; D-14 SmartScreen note captured
metrics:
  duration: ~8 minutes
  completed: "2026-07-16"
---

# Phase 03 Plan 03: First Distributable CI Pipeline Summary

**One-liner:** Velopack vpk packaging + conditional Authenticode signtool signing step wired into Windows CI job, producing a downloadable Setup.exe artifact on every push.

## What Was Built

Extended `.github/workflows/ci.yml` Windows job with four steps appended after the existing `cargo test` step:

1. **Install vpk CLI** — `dotnet tool install -g vpk --version "1.*"` installs the Velopack packaging tool from the dotnet tool registry.

2. **Package with vpk** — PowerShell step extracts the version from `cargo metadata`, then calls `vpk pack --packId BrevlyPrint --packVersion $version --packDir target\release --mainExe brevly-print.exe --outputDir Releases --packTitle "Brevly Print" --packAuthors "Brevly"`. Produces `Releases/*Setup.exe`.

3. **Sign Setup.exe (conditional)** — Gated on `${{ secrets.CODESIGN_PFX_BASE64 != '' }}`. When the OV certificate secret is present, decodes base64 PFX to `$env:RUNNER_TEMP\cert.pfx`, calls `signtool sign /fd SHA256 /f ... /tr http://timestamp.digicert.com /td SHA256`, then removes the temp cert file. Skips cleanly on all builds until OV cert is procured (D-12/D-13).

4. **Upload artifact** — `actions/upload-artifact@v4` uploads `Releases/*Setup.exe` as `brevly-print-setup` artifact, downloadable from the GitHub Actions run.

Also updated `.planning/STATE.md` Open Todos:
- OV cert todo now names both GitHub secrets required (`CODESIGN_PFX_BASE64`, `CODESIGN_PFX_PASSWORD`) and explicitly ties SC-4 sign-off to cert procurement.
- Added D-14 SmartScreen warm-up expectation: new OV-signed binary shows initial warnings until download volume builds (~hundreds of clean installs).

## Tasks Completed

| Task | Name | Commit | Files Modified |
|------|------|--------|----------------|
| 1 | Extend .github/workflows/ci.yml with vpk + signtool + artifact upload | dc5bb01 | .github/workflows/ci.yml |
| 2 | Update STATE.md — OV cert blocker + SmartScreen note (D-14) | f51d0bc | .planning/STATE.md |

## Verification

- `grep "vpk pack" .github/workflows/ci.yml` → line 82
- `grep "CODESIGN_PFX_BASE64" .github/workflows/ci.yml` → lines 94, 97, 100
- `grep "signtool sign /fd SHA256" .github/workflows/ci.yml` → line 104
- `grep "upload-artifact@v4" .github/workflows/ci.yml` → line 109 with `name: brevly-print-setup`
- `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"` → exits 0 (YAML VALID)
- Linux job (`ubuntu-latest`) contains no vpk or signtool lines — unchanged
- `grep "CODESIGN_PFX_BASE64" .planning/STATE.md` → line 88
- `grep "SmartScreen reputation" .planning/STATE.md` → line 89

## Deviations from Plan

None — plan executed exactly as written.

## Known Stubs

None. The signing step skipping when the secret is absent is intentional (D-12/D-13), not a stub — the pipeline is complete and correct. The OV cert procurement is tracked as an external blocker in STATE.md.

## Checkpoint Pending

Task 3 is a `checkpoint:human-verify` (gate="blocking-human") requiring Windows runtime verification (tray icon, single-instance guard, reboot survival) and CI packaging verification (push branch to see brevly-print-setup artifact). See plan 03-03-PLAN.md for detailed verification steps.

## Threat Surface

All threats handled by plan's threat model:
- T-03-03-01: PFX decoded to `$env:RUNNER_TEMP\cert.pfx` and deleted immediately after signing
- T-03-03-04: GitHub Actions secrets not exposed on PR runs (GitHub default); signtool step will skip on PR CI

No new threat surfaces beyond what the plan's `<threat_model>` covers.

## Self-Check: PASSED

Files verified:
- FOUND: .github/workflows/ci.yml (modified)
- FOUND: .planning/STATE.md (modified)

Commits verified:
- FOUND: dc5bb01 (Task 1 — CI pipeline)
- FOUND: f51d0bc (Task 2 — STATE.md)
