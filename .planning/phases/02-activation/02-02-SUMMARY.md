---
phase: 02-activation
plan: "02"
subsystem: printer-hardware-impl
tags: [windows-only, cfg-gated, writePrinter, serialport, RAW-datatype, C1-critical]
dependency_graph:
  requires:
    - printer::Printer trait (02-01)
    - printer::PrinterError variants (02-01)
    - printer::PrinterEntry + PrinterId (02-01)
    - windows 0.62 Win32_Graphics_Printing dep (Cargo.toml, 02-01)
    - serialport 4.9 dep (Cargo.toml, 02-01)
  provides:
    - printer::spooler::WindowsSpoolerPrinter — WritePrinter RAW spooler path
    - printer::serial::SerialPrinter — serialport COM write path
    - windows_enumerate_printers() — combined spooler + COM list (USB/Serial labels)
    - printer_from_entry() — factory dispatching to spooler/serial impls
  affects:
    - src/printer/spooler.rs — full Win32 WritePrinter RAW sequence (was todo!() stub)
    - src/printer/serial.rs — serialport::new().open() + write_all (was todo!() stub)
    - src/printer/mod.rs — doc comments updated; Windows branches verified complete
tech_stack:
  added: []
  patterns:
    - Win32 unsafe FFI with unconditional ClosePrinter (T-02-06 mitigated)
    - Two-function split (write_raw_to_spooler + submit_job) for clean handle lifetime
    - #![cfg(windows)] file-level gate (dpapi.rs precedent)
key_files:
  created: []
  modified:
    - src/printer/spooler.rs (todo!() stub → full Win32 WritePrinter RAW impl)
    - src/printer/serial.rs (todo!() stub → serialport::new().open() + write_all)
    - src/printer/mod.rs (stale doc comments updated; Windows branches already complete)
decisions:
  - "Two-function split (write_raw_to_spooler outer + submit_job inner) ensures ClosePrinter runs on every exit path, mitigating T-02-06 handle leak threat"
  - "BOOL return checked for WritePrinter (Win32 convention: 0 = failure; BOOL is not a Result in the windows crate)"
  - "windows::core::PWSTR::null() used for pOutputFile per windows 0.62 PWSTR API"
  - "mod.rs Windows branches were already fully implemented in Plan 01 (deviation from plan); Task 2 scope was doc cleanup and verification"
metrics:
  duration: "~7 minutes"
  completed: "2026-07-16"
  tasks_completed: 2
  tasks_total: 2
  files_created: 0
  files_modified: 3
---

# Phase 02 Plan 02: Windows Hardware Printer Paths Summary

**One-liner:** Win32 WritePrinter RAW spooler path + serialport COM write path behind `#![cfg(windows)]`, with C1 pDatatype="RAW" baked in and T-02-06 handle-leak mitigated via unconditional ClosePrinter.

## Tasks Executed

| Task | Name | Commit | Status |
|------|------|--------|--------|
| 1 | spooler.rs — WritePrinter RAW + serial.rs — serialport write | 19c26eb | Complete |
| 2 | Verify windows_enumerate_printers() + printer_from_entry() Windows branches | 130166d | Complete |

## What Was Built

### Task 1: spooler.rs — WindowsSpoolerPrinter (WritePrinter RAW path)

`src/printer/spooler.rs` implements the full Win32 sequence per RESEARCH.md Pattern 3:

- `WindowsSpoolerPrinter { pub printer_name: String }` with `pub fn new(String) -> Self`
- `impl Printer for WindowsSpoolerPrinter` delegates to `unsafe fn write_raw_to_spooler()`
- Win32 sequence: `OpenPrinterW` → `submit_job(handle)` → `ClosePrinter(handle)` (unconditional)
- `submit_job`: `StartDocPrinterW(level=1, DOC_INFO_1W)` → `StartPagePrinter` → `WritePrinter` → `EndPagePrinter` → `EndDocPrinter`
- **CRITICAL C1**: `DOC_INFO_1W.pDatatype = "RAW\0"` (UTF-16, annotated as load-bearing)
- **T-02-06 mitigated**: `submit_job` is a separate inner function so `ClosePrinter` in the outer function is reached unconditionally regardless of error path
- `OpenPrinterW` error maps to `PrinterError::NotFound`; other Win32 errors map to `PrinterError::PrintFailed`
- `WritePrinter` uses BOOL return-value check (not `?`) per Win32 convention
- Printer name passed verbatim to `OpenPrinterW` (Pitfall 5)

`src/printer/serial.rs` implements the COM port write path:

- `SerialPrinter { pub port_name: String }` with `pub fn new(String) -> Self`
- `impl Printer for SerialPrinter`: `serialport::new(&self.port_name, 9600).open()` → `write_all(bytes)`
- Open error maps to `PrinterError::SerialPort`; write error maps to `PrinterError::Io`

### Task 2: mod.rs Windows branches verified complete

`src/printer/mod.rs` Windows branches were already fully implemented in Plan 01 (see deviation below).

Acceptance criteria verified:
- `windows_enumerate_printers()`: calls `get_printers()` + `available_ports()`, formats `"{} (USB)"` and `"{} (Serial)"` labels, flags `is_default` from `get_default_printer()`
- `printer_from_entry()`: `PrinterId::Spooler(name)` → `WindowsSpoolerPrinter::new(name)`, `PrinterId::Serial(port)` → `SerialPrinter::new(port)`
- No `todo!()` macro calls in Windows branches
- Doc comments updated to remove stale "Plan 02 implements" and "todo!()" references

## Verification Results

- `cargo build` (Linux): **PASS** (all Windows files excluded via `#![cfg(windows)]`)
- `cargo test` (Linux): **16/16 PASS**, 1 ignored (Windows-only Phase 1 test)
- Source assertion: all acceptance criteria verified by grep (RAW literal, WritePrinter, EndDocPrinter, get_printers, available_ports, (USB)/(Serial) labels)
- Windows build/test-print: **Manual checkpoint — Plan 02-03** (Windows hardware required)

## Deviations from Plan

### Structural deviation (inherited from Plan 01)

**1. Task 2: windows_enumerate_printers() + printer_from_entry() already complete**

- **Found during:** Task 2 pre-flight read of mod.rs
- **Issue:** Plan 02-02 Task 2 planned to "replace the Plan 01 `todo!()` in `windows_enumerate_printers()`" — but Plan 01 already implemented the full Pattern 7 body inline (including `get_printers()`, `available_ports()`, format labels, and the `printer_from_entry()` Windows dispatch). This was documented in Plan 01 SUMMARY as "Structural deviation 1."
- **Action:** Task 2 scope reduced to: (a) verify all acceptance criteria pass by grep, (b) remove stale doc comments referencing "Plan 02 implements" and "todo!()", (c) verify `cargo test` still green.
- **Files modified:** `src/printer/mod.rs` (doc comment update only)
- **Commit:** 130166d

### Auto-fixed (Rule 2 — missing correctness detail)

None — no Rule 1/2/3 auto-fixes were needed.

## Known Stubs

None — all stubs from Plan 01 have been filled:

| File | Was | Now |
|------|-----|-----|
| `src/printer/spooler.rs` | `todo!()` stub | Full Win32 WritePrinter RAW sequence |
| `src/printer/serial.rs` | `todo!()` stub | serialport::new().open() + write_all |

## Threat Surface Scan

No new network endpoints or trust boundaries introduced. All code is local hardware I/O.

| Flag | File | Description |
|------|------|-------------|
| T-02-06 mitigated | src/printer/spooler.rs | ClosePrinter unconditional via submit_job split |
| T-02-05 mitigated | src/printer/spooler.rs | pDatatype="RAW" enforced (C1); Windows manual test-print is runtime gate |
| T-02-07 accepted | src/printer/spooler.rs | unsafe FFI; data.len() from trusted caller; local agent |

## Self-Check: PASSED

Files modified:
- [x] src/printer/spooler.rs — FOUND, contains RAW, WritePrinter, EndDocPrinter, #![cfg(windows)]
- [x] src/printer/serial.rs — FOUND, contains serialport::new, write_all, #![cfg(windows)]
- [x] src/printer/mod.rs — FOUND, contains get_printers, available_ports, (USB)/(Serial) labels, no todo!()

Commits verified:
- [x] 19c26eb — feat(02-02): Task 1 (spooler.rs + serial.rs)
- [x] 130166d — feat(02-02): Task 2 (mod.rs doc cleanup)
