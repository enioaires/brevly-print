---
phase: 07-auto-update-distribution-polish
plan: "03"
subsystem: ci
tags: [ci, velopack, distribution, update-package]
dependency_graph:
  requires: ["07-01"]
  provides: ["Velopack update package (releases.win.json + .nupkg) from CI", "Noren /api/agent/version payload (version/downloadUrl/sha256)"]
  affects: [".github/workflows/ci.yml"]
tech_stack:
  added: []
  patterns: ["vpk pack --channel win --delta BestSpeed", "assets.win.json parsing in PowerShell", "GITHUB_OUTPUT multi-value write"]
key_files:
  created: []
  modified:
    - .github/workflows/ci.yml
decisions:
  - "Extract step uses assets.win.json sha256 field with Get-FileHash fallback (RESEARCH.md A5)"
  - "Upload artifact renamed to brevly-print-release to reflect full release set"
  - "S3/Cloudflare upload intentionally absent — Noren-backend dependency (D-06)"
  - "Signing gate (CODESIGN_PFX_BASE64) left completely unchanged from Phase 3"
metrics:
  duration: "< 5 minutes"
  completed: "2026-07-16"
  tasks_completed: 2
  tasks_total: 2
---

# Phase 7 Plan 3: CI Update Package + Noren Payload Summary

**One-liner:** CI Windows job now produces the full Velopack update package (releases.win.json + .nupkg via `--channel win --delta BestSpeed`) and surfaces `{version, downloadUrl, sha256}` to GITHUB_OUTPUT for Noren's `/api/agent/version` endpoint.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Extend vpk pack with --channel win + --delta BestSpeed | 50eca07 | .github/workflows/ci.yml |
| 2 | Surface Noren /api/agent/version payload + upload update artifacts | 27c4f36 | .github/workflows/ci.yml |

## What Was Built

### Task 1 — vpk pack extended

The existing `Package with vpk` step was extended with two new flags:
- `--channel win`: causes `vpk pack` to emit `releases.win.json` alongside `Setup.exe`. This feed file is what the agent's `HttpSource` in plan 02 consumes.
- `--delta BestSpeed`: causes `vpk pack` to produce a delta `.nupkg` when a previous release artifact exists in `Releases/`. On the first CI run (no previous `.nupkg` in `Releases/`) no delta is generated — this is expected (Pitfall 7), not a failure. A YAML comment captures this.

### Task 2 — Extract step + extended upload

A new `Extract update artifact info for Noren /api/agent/version endpoint` step (id: `update_info`) was inserted between the signing step and the upload step. It:
1. Reads `Releases/assets.win.json` (produced by `vpk pack`) and selects the `Full` asset entry.
2. Writes `version`, `filename`, and `sha256` to `$env:GITHUB_OUTPUT` — making them available to downstream CI steps.
3. Includes a `Get-FileHash` fallback if `assets.win.json` lacks a `sha256` field (per RESEARCH.md Assumption A5).
4. Echoes a human-readable `{ version, downloadUrl, sha256 }` line so the Noren backend team can directly copy the endpoint payload shape.

The upload step was extended from uploading only `*Setup.exe` to also including `*.nupkg`, `releases.win.json`, and `assets.win.json`. The artifact name was renamed from `brevly-print-setup` to `brevly-print-release` to reflect the full release set.

The signing step gate (`if: ${{ env.CODESIGN_PFX_BASE64 != '' }}`) was left completely unchanged.

No S3/Cloudflare upload or `vpk upload` command was added — this remains the Noren-backend dependency (D-06).

## Deviations from Plan

None — plan executed exactly as written.

The only non-obvious choice: the comment inside the `run:` block originally referenced literal text `vpk upload / aws s3 / wrangler` which would have caused the plan's own acceptance-criteria grep to trigger a false positive. The comment was rephrased to `The upload step is intentionally absent here` — equivalent meaning, no behavioral change.

## Threat Surface Scan

No new network endpoints, auth paths, or trust boundaries introduced. The CI job produces artifacts at the same trust level as the existing `Setup.exe` upload. The `sha256` surfaced in `GITHUB_OUTPUT` is the authoritative hash the agent verifies per T-7-CI-integrity in the plan's threat model — CI is the source of truth for this value, matching plan 01/02's `verify_sha256` check.

## Self-Check

### Files

- [x] `.github/workflows/ci.yml` modified (both tasks committed)

### Commits

- [x] 50eca07 — Task 1 commit exists
- [x] 27c4f36 — Task 2 commit exists

### Verification passes

- [x] `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml')); print('yaml-ok')"` → `yaml-ok`
- [x] `grep -q "channel win"` → present
- [x] `grep -q "delta BestSpeed"` → present (`flags-present`)
- [x] `grep -q "GITHUB_OUTPUT"` → present
- [x] `grep -q "assets.win.json"` → present
- [x] `grep -q "releases.win.json"` → present (`publish-loop-present`)
- [x] Signing gate `CODESIGN_PFX_BASE64 != ''` unchanged → `signing-gate-intact`
- [x] `*.nupkg` in upload path → `nupkg-in-upload`
- [x] No `vpk upload / aws s3 / wrangler` command → `no-forbidden-upload`

## Self-Check: PASSED
