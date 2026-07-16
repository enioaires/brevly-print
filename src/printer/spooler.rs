//! Windows spooler printer implementation (WritePrinter RAW path).
//!
//! **Windows-only.** This module is compiled only when `cfg(windows)`.
//!
//! Uses the Win32 `OpenPrinterW` / `StartDocPrinterW` / `WritePrinter` sequence to send
//! raw ESC/POS bytes directly to a USB thermal printer via the Windows print spooler.
//!
//! CRITICAL PITFALL C1: `pDatatype` in `DOC_INFO_1W` MUST be set to `"RAW"` or the spooler
//! will interpret ESC/POS bytes as GDI/EMF and produce silent garbage output.
//!
//! **Plan 02 implements** the full Windows spooler body.
//! This file is a compile-verified stub — `todo!()` is intentional and annotated.
#![cfg(windows)]

use super::{Printer, PrinterError};

/// Windows spooler printer — sends raw ESC/POS bytes via `WritePrinter` with `"RAW"` datatype.
///
/// Created by `printer_from_entry()` for `PrinterId::Spooler(name)`.
pub struct WindowsSpoolerPrinter {
    /// Windows printer name as returned by `printers::get_printers()` → `p.name`.
    /// Must be passed verbatim to `OpenPrinterW` (Pitfall 5).
    printer_name: String,
}

impl WindowsSpoolerPrinter {
    pub fn new(printer_name: String) -> Self {
        Self { printer_name }
    }
}

impl Printer for WindowsSpoolerPrinter {
    fn print_raw(&self, _bytes: &[u8]) -> Result<(), PrinterError> {
        // Plan 02 implements: OpenPrinterW → StartDocPrinterW("RAW") → WritePrinter → EndDocPrinter
        // CRITICAL (C1): pDatatype MUST be "RAW" — see RESEARCH.md Pattern 3 and Pitfall 1.
        todo!("Plan 02 implements WindowsSpoolerPrinter::print_raw (printer name: {})", self.printer_name)
    }
}
