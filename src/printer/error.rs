//! Printer error type — shared across all platform implementations.
//!
//! Compiled on all platforms (no cfg-gate) so the Linux stub and the Windows
//! spooler/serial impls share the same typed error contract.

use thiserror::Error;

/// Errors that can occur when enumerating or printing.
///
/// Mirrors the shape of `CredentialError` (thiserror 2, `#[from]` for I/O).
#[derive(Error, Debug)]
pub enum PrinterError {
    /// The named printer or COM port was not found or is not accessible.
    #[error("Printer not found or not accessible: {0}")]
    NotFound(String),

    /// The print job was submitted but the spooler reported a failure.
    #[error("Print job failed: {0}")]
    PrintFailed(String),

    /// Serial port error (COM port path).
    #[error("Serial port error: {0}")]
    SerialPort(String),

    /// I/O error while communicating with the printer.
    #[error("I/O error")]
    Io(#[from] std::io::Error),
}
