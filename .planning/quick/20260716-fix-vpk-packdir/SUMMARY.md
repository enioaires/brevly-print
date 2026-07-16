---
slug: fix-vpk-packdir
status: complete
completed: "2026-07-16"
---

# Summary: Fix vpk --packDir path

Changed `--packDir target\release` to `--packDir target\x86_64-pc-windows-msvc\release`
in `.github/workflows/ci.yml` line 88.

The build step uses `--target x86_64-pc-windows-msvc` so the binary is placed under the
target-triple subdirectory, not the bare `target/release/` path vpk was searching.

Commit: a743a74
