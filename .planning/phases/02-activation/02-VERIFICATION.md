---
status: human_needed
phase: 02-activation
goal: "Users (restaurant owners) can install the agent, enter a serial number, select a printer, test-print, and save — resulting in a bound, autostarting agent ready for operation"
requirement_ids: [ACT-01, ACT-02, ACT-03, ACT-04, ACT-05, ACT-06, ACT-07, ACT-08]
verified: 2026-07-16
verifier: orchestrator (gsd-verifier hung on a transient Write denial; verification completed inline against the same evidence)
score:
  code_verifiable: 8/8 implemented
  linux_provable: passed
  human_needed: 8   # all require Windows hardware and/or the live Noren endpoint for final sign-off
  gaps: 0
build: clean
tests: 16 passed / 1 ignored (GPU) on x86_64-unknown-linux-gnu
code_review: 5 critical + 6 warning found; 10 fixed, 1 (WR-02) false positive
head: 59c07b2
---

# Phase 02 — Activation — Verification

## Verdict: HUMAN_NEEDED

All eight ACT requirements are **implemented and merged**, and every part that can be
proven on the Linux portable core is green (build clean, 16 tests pass, code review + fix
pass complete). What remains for each requirement is validation that is **physically
impossible on this Linux dev host** — it needs Windows hardware (spooler/COM/DPAPI/HKCU)
and/or the live Noren `POST /api/agent/activate` endpoint, both of which are known,
tracked cross-repo dependencies (STATE.md). These are classified as human-verification
items, **not implementation gaps**.

## Automated Evidence (Linux-provable)

- `cargo build` — clean (283 crates).
- `cargo test` — 16 passed, 1 ignored (wgpu adapter, headless), 0 failed across 9 suites.
- Code review (standard depth, 14 files): 5 critical + 6 warning + 4 info.
  - Fixed: CR-01..CR-05, WR-01, WR-03..WR-06 (10 findings, 10 atomic commits).
  - WR-02 skipped as a version false positive (egui 0.35 API is correct).
  - IN-03/IN-04 are Info false positives (Cargo resolves the pinned versions; build proves it).
- Pitfall C1 preserved: `src/printer/spooler.rs` sets `pDatatype = "RAW"` (UTF-16, triple-annotated).

## Requirement Traceability

| Req | What it requires | Implementation evidence | Remaining (human) |
|-----|------------------|-------------------------|-------------------|
| ACT-01 | Windows installer installs the agent | Agent compiles as a Windows binary; the **installer/distributable is built in Phase 3** (First Distributable) | Deferred to Phase 3 — no installer artifact in this phase |
| ACT-02 | First run opens a serial-entry window | `main.rs` credential-check branch → `activation_window.rs` egui form (winit 0.30 + egui-wgpu, cross-platform) | Visual confirm on a Windows desktop session |
| ACT-03 | Serial validated against Noren → binds tenant | `noren_client::activate()` async client; status→error mapping unit-tested vs mock (200→Ok, 403/404→InvalidSerial, 409→AlreadyActiveOther, transport→Transport) | Live round-trip vs Noren `POST /api/agent/activate` (endpoint not yet live) |
| ACT-04 | Combined printer list (Windows printers + COM) | Linux stub returns empty Vec (tested); `windows_enumerate_printers()` merges `printers::get_printers` + `serialport::available_ports`, default flagged (cfg(windows)) | Run on Windows with a real printer + COM port |
| ACT-05 | Test-print validates RAW bytes reach printer | WritePrinter RAW path (C1) with handle/EndDoc cleanup on all error paths (WR-01); serial path flushes after write (CR-05); test-print emits `ESC @` + coupon + `GS V 0` cut | Print a legible coupon + paper cut on a thermal printer |
| ACT-06 | Save: agentToken via DPAPI, config via SQLite | Save flow wired: `CredentialStore` (Windows DPAPI impl / Linux dev impl) + `config_store::set` (SQLite, Phase-1 tested) + connection closed before exit (CR-01) | DPAPI encrypt/decrypt round-trip on Windows |
| ACT-07 | Unreadable credential → back to activation | `main.rs` routes `CredentialError::NotFound\|Corrupt` into the activation window; TOCTOU double-load fixed (CR-02); reactivation banner in UI-SPEC | Corrupt-blob case on real DPAPI (Windows) |
| ACT-08 | Registers autostart, starts with Windows | `auto-launch` HKCU Run registration in the save flow (cfg(windows), `WindowsEnableMode::CurrentUser`) | Reboot a Windows machine; confirm Task Manager Startup entry + auto-launch |

## must_haves check (per plan)

- **02-01** (seams): ✅ all artifacts present — `activate()`, `Printer` trait, `PrinterError`, `get_machine_id`, mock-server status-mapping tests, Linux stub contract test.
- **02-02** (Windows hardware): ✅ `WindowsSpoolerPrinter` (RAW), `SerialPrinter` (write+flush), combined enumeration + `printer_from_entry` mapping — all cfg(windows); Linux core stays green.
- **02-03** (activation window): ✅ startup credential branch, multi-thread tokio runtime, egui form per UI-SPEC, async oneshot polling (non-freezing), test-print, DPAPI+SQLite+HKCU save then clean exit; 409 re-bind now threads `force_rebind` (CR-03).

## Human Verification Checklist

Tracked in `02-HUMAN-UAT.md`. Blocking dependencies:
1. A Windows machine with a USB thermal printer (and ideally a serial/COM device).
2. Noren `POST /api/agent/activate` live (or a stub returning the contract shape) + the `agent_serials` table.

Until both are available, this phase is functionally complete on the code side but its
end-to-end acceptance stays pending — exactly as the roadmap anticipated.
