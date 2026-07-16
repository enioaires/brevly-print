//! Linux stub printer — DEV/TEST ONLY. Never ships.
//!
//! Exists to exercise the `Printer` trait contract on Linux without any
//! hardware. `StubPrinter::print_raw()` is a no-op that always returns `Ok`.
//!
//! Mirrors `src/credential_store/devfile.rs` (the DEV/TEST credential stub).
#![cfg(not(windows))]

use super::{Printer, PrinterError};

/// No-op printer for Linux dev/test. Always succeeds. No hardware required.
pub struct StubPrinter;

impl Printer for StubPrinter {
    fn print_raw(&self, _bytes: &[u8]) -> Result<(), PrinterError> {
        Ok(()) // No-op: Linux dev only
    }
}
