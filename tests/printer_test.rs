//! Integration tests for the `Printer` trait contract via the Linux stub.
//!
//! **Linux-provable** — tests the stub path, which requires no hardware.
//! Mirrors `tests/credential_contract_test.rs` (same pattern: trait contract + error
//! variants, stub implementation on non-Windows).
//!
//! Covers Plan 02-01 Task 2 acceptance criteria:
//!   - `enumerate_printers()` returns an empty Vec on Linux.
//!   - `StubPrinter::print_raw()` returns `Ok` (no-op, no hardware needed).

use brevly_print::printer::{enumerate_printers, printer_from_entry, PrinterId};

/// Verify `enumerate_printers()` returns an empty list on Linux.
///
/// On Windows this would return installed printers + COM ports.
/// On Linux the cfg-gated stub always returns `vec![]`.
#[cfg(not(windows))]
#[test]
fn test_enumerate_printers_empty_on_linux() {
    // cfg(not(windows)) → stub path → always empty
    let entries = enumerate_printers();
    assert!(
        entries.is_empty(),
        "Linux stub must return empty printer list, got: {} entries",
        entries.len()
    );
}

/// Verify `StubPrinter::print_raw()` returns `Ok` with no hardware.
///
/// On Linux, `printer_from_entry()` always returns a `StubPrinter` regardless
/// of the `PrinterId` variant — the stub is a no-op.
#[cfg(not(windows))]
#[test]
fn test_stub_printer_print_raw_returns_ok() {
    // Use a Spooler id — on Linux, printer_from_entry() returns StubPrinter for any id
    let id = PrinterId::Spooler("TEST_PRINTER".to_string());
    let printer = printer_from_entry(&id);
    let result = printer.print_raw(b"\x1b\x40\x1d\x56\x00");
    assert!(
        result.is_ok(),
        "StubPrinter::print_raw should return Ok, got: {result:?}"
    );
}

/// Verify `StubPrinter` works with a Serial ID too.
#[cfg(not(windows))]
#[test]
fn test_stub_printer_serial_print_raw_returns_ok() {
    let id = PrinterId::Serial("COM3".to_string());
    let printer = printer_from_entry(&id);
    let result = printer.print_raw(b"\x1b\x40Hello\x1d\x56\x00");
    assert!(
        result.is_ok(),
        "StubPrinter::print_raw (serial id) should return Ok, got: {result:?}"
    );
}
