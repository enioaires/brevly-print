---
phase: 01-foundation-thread-model-spike
plan: "03"
subsystem: event-loop-and-render
tags: [winit, egui, egui-winit, egui-wgpu, wgpu, walking-skeleton, spike, cross-platform, checkpoint]

dependency_graph:
  requires:
    - phase: "01-01"
      provides: "Cargo lib+bin scaffold, app_dir.rs, CredentialStore trait + cfg impls, target-gated deps"
    - phase: "01-02"
      provides: "ConfigStore (migration v1, 3 tables, get/set), both credential impls, contract tests"
  provides:
    - "src/main.rs: winit 0.30 ApplicationHandler event loop; startup wiring (app dir → migrate state.db → config write/read → credential round-trip) → opens window"
    - "src/spike_window.rs: raw egui via egui-winit + egui-wgpu; interactive frame (text field + Aplicar button → label); format from surface caps (not hardcoded)"
    - "tests/window_smoke_test.rs: headless-safe wgpu adapter smoke test (#[ignore])"
    - "SC-1 proven: winit 0.30 ApplicationHandler drives raw egui, no separate Win32 loop, no tao, no eframe"
---

# Summary — Plan 01-03: Walking Skeleton (winit 0.30 + raw egui)

## Outcome

**Embedded approach cleared the D-09 bar — no D-10 subprocess fallback needed.**

A single binary (`cargo run`) drives a `winit 0.30` `ApplicationHandler` event loop rendering a
raw-`egui` window through `egui-winit` + `egui-wgpu`, with **no separate Win32 message loop, no
`tao`, no `eframe`**. This directly retires pitfall **C2** (eframe/tao event-loop conflict) — the
gate that blocked all subsequent GUI work.

At startup the binary exercises the full persistence stack end-to-end before opening the window:
1. Creates the app dir (`dirs::data_dir()/BrevlyPrint/`)
2. Migrates `state.db` to `user_version=1` (3 tables: `config`, `printed_jobs`, `retry_queue`)
3. Writes + reads back a `config` row (`skeleton_probe = "ok"`)
4. Round-trips a dummy credential through the `CredentialStore` trait
5. Opens an interactive window (text field + "Aplicar" button → "Aplicado:" label)

## Checkpoint (Task 3 — blocking human-verify): APPROVED

Owner-verified **on Linux (primary proof) AND confirmed on Windows**:

- **Linux (dev box, Arch):** `cargo run` — startup logs report app dir, `config` write/read
  (`skeleton_probe = Some("ok")`), `Credential round-trip: OK`; window opens
  (`format=Rgba8UnormSrgb`); typing + "Aplicar" updates the label; clean exit. Screenshot evidence
  provided by owner.
- **Windows (owner box):** `cargo run` confirmed — window renders under **DX12**, credential
  round-trip uses **real DPAPI (`Scope::User`)**, interaction works.

No embedding wall was hit; the D-10 subprocess fallback was **not** taken. The embedded
winit+egui window is therefore the confirmed approach carried into Phase 2 (activation window).

## wgpu Backend (Linux)

`wgpu::Instance::default()` picks all available backends; `PowerPreference::default()`;
`Limits::downlevel_defaults()` (keeps the GL/low-end path viable). Surface texture format is taken
from `surface.get_capabilities(&adapter).formats[0]` — **not hardcoded** (OQ2 resolved). On the
Linux dev box this resolved to **`Rgba8UnormSrgb`** (Vulkan). On Windows the same code path selects
DX12 with its surface-preferred format. Cross-platform window proven on both.

## How to run the ignored wgpu smoke test

The render/adapter test is `#[ignore]` so default `cargo test` stays headless-CI safe (no GPU or
software rasterizer required):

```bash
# Local machine with a real GPU or software rasterizer (lavapipe on Linux / WARP on Windows):
cargo test -- --ignored                 # runs test_wgpu_adapter_available
# or target it directly:
cargo test --test window_smoke_test -- --ignored
```

Default `cargo test` (no `--ignored`) does **not** touch the wgpu adapter — CI gate unaffected.

## Verification (all green on Linux dev box)

- `cargo build` → exits 0 (egui-winit + egui-wgpu integration compiles)
- `cargo test` → all pass: credential contract (2), config store (4), credential-store DPAPI file
  (cfg-gated, 0 on Linux), window smoke (1 ignored). No failures.
- Manual checkpoint: window opens, text+button→label works, startup logs prove all store
  round-trips, clean exit — on Linux, confirmed on Windows.

## Files Touched

| File | Role |
|------|------|
| `src/main.rs` | winit `ApplicationHandler` event loop + startup store wiring (velopack no-op bootstrapper entry is Windows-gated) |
| `src/spike_window.rs` | raw egui render integration (Instance→adapter→device→surface→egui-wgpu renderer), interactive spike UI |
| `tests/window_smoke_test.rs` | headless-safe `#[ignore]` wgpu adapter smoke test |

## Notes for Later Phases

- The spike window grows into the **Phase 2** activation screen (serial input, printer/COM
  dropdown, test-print) — same event loop, no architectural change.
- The `CredentialStore` cfg-gated pattern is the precedent for the **Phase 5** `Printer` trait
  (WritePrinter RAW / serial).
- Phase 1 credential value is a hardcoded dummy (`b"skeleton-dummy"`); real tokens arrive in Phase 2.
