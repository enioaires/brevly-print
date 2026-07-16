//! Windows serial-port printer implementation.
//!
//! **Windows-only.** This module is compiled only when `cfg(windows)`.
//!
//! Sends raw ESC/POS bytes to a COM port (e.g. `"COM3"`) using the `serialport` crate.
//! The port is opened at 9600 baud (default for most thermal printers on serial).
//!
//! **Plan 02 implements** the full serial body.
//! This file is a compile-verified stub — `todo!()` is intentional and annotated.
#![cfg(windows)]

use super::{Printer, PrinterError};

/// Serial-port printer — sends raw ESC/POS bytes to a COM port.
///
/// Created by `printer_from_entry()` for `PrinterId::Serial(port_name)`.
pub struct SerialPrinter {
    /// COM port name, e.g. `"COM3"`.
    port_name: String,
}

impl SerialPrinter {
    pub fn new(port_name: String) -> Self {
        Self { port_name }
    }
}

impl Printer for SerialPrinter {
    fn print_raw(&self, _bytes: &[u8]) -> Result<(), PrinterError> {
        // Plan 02 implements: serialport::new(port_name, 9600).open() → write_all(bytes)
        todo!("Plan 02 implements SerialPrinter::print_raw (port: {})", self.port_name)
    }
}
