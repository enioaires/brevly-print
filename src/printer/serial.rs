//! Windows serial-port printer implementation.
//!
//! **Windows-only.** This module is compiled only when `cfg(windows)`.
//!
//! Sends raw ESC/POS bytes to a COM port (e.g. `"COM3"`) using the `serialport` crate.
//! The port is opened at 9600 baud (default for most thermal printers on serial).
#![cfg(windows)]

use std::io::Write as _;

use super::{Printer, PrinterError};

/// Serial-port printer — sends raw ESC/POS bytes to a COM port.
///
/// Created by `printer_from_entry()` for `PrinterId::Serial(port_name)`.
pub struct SerialPrinter {
    /// COM port name, e.g. `"COM3"`.
    pub port_name: String,
}

impl SerialPrinter {
    pub fn new(port_name: String) -> Self {
        Self { port_name }
    }
}

impl Printer for SerialPrinter {
    fn print_raw(&self, bytes: &[u8]) -> Result<(), PrinterError> {
        // Open the COM port at 9600 baud (most thermal printers default to 9600).
        let mut port = serialport::new(&self.port_name, 9600)
            .open()
            .map_err(|e| PrinterError::SerialPort(format!("{}: {e}", self.port_name)))?;

        // Write all ESC/POS bytes — serialport implements std::io::Write.
        port.write_all(bytes)
            .map_err(PrinterError::Io)?;

        Ok(())
    }
}
