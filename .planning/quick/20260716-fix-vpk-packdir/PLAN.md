---
slug: fix-vpk-packdir
created: "2026-07-16"
status: in-progress
---

# Fix vpk --packDir path in ci.yml

## Goal

CI "Package with vpk" step fails because `--packDir target\release` doesn't exist when
the build uses `--target x86_64-pc-windows-msvc` (output lands in
`target\x86_64-pc-windows-msvc\release\` instead).

## Root Cause

Line 88 of `.github/workflows/ci.yml`:
```
--packDir target\release `
```
Should be:
```
--packDir target\x86_64-pc-windows-msvc\release `
```

## Task

1. Edit `.github/workflows/ci.yml` line 88: change `--packDir target\release` → `--packDir target\x86_64-pc-windows-msvc\release`
2. Commit with message: `fix(ci): correct vpk --packDir path for cross-target build`
