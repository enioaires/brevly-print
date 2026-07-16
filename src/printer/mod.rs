//! Printer: trait + cfg-gated platform implementations.
//!
//! On Windows: `spooler.rs` (WritePrinter RAW via Win32) and `serial.rs` (COM port via serialport).
//! On Linux/non-Windows: `stub.rs` (no-op — DEV/TEST ONLY).
//!
//! Mirrors `src/credential_store/mod.rs` (the cfg-gated trait + factory precedent).

pub mod error;

#[cfg(windows)]
mod spooler;

#[cfg(windows)]
mod serial;

#[cfg(not(windows))]
mod stub;

pub use error::PrinterError;

/// Entry in the combined printer list shown to the user.
///
/// On Windows, populated from `printers::get_printers()` (spooler) and
/// `serialport::available_ports()` (COM ports). On Linux, always empty.
#[derive(Debug, Clone)]
pub struct PrinterEntry {
    /// Human-readable label shown in the UI: e.g. "EPSON TM-T20 (USB)" or "COM3 (Serial)".
    pub display_name: String,
    /// Internal identifier passed to the platform impl.
    pub id: PrinterId,
    /// Whether this is the Windows default printer (pre-selected in the UI dropdown — D-06).
    pub is_default: bool,
}

/// Platform-specific printer identifier.
#[derive(Debug, Clone)]
pub enum PrinterId {
    /// Windows spooler printer name (as registered in Windows).
    Spooler(String),
    /// COM port name, e.g. `"COM3"`.
    Serial(String),
}

/// Abstraction over platform-specific raw-byte printing.
///
/// Mirrors `CredentialStore` (same trait shape, cfg-gated impls).
pub trait Printer {
    /// Send raw ESC/POS bytes to the printer.
    ///
    /// On Windows spooler path: CRITICAL — pDatatype must be "RAW" (Pitfall C1).
    /// On Linux stub: always returns `Ok(())`.
    fn print_raw(&self, bytes: &[u8]) -> Result<(), PrinterError>;
}

/// Return a combined list of available printers.
///
/// On Windows: spooler printers (USB) + COM ports (serial).
/// On Linux/non-Windows: always returns an empty `Vec` (Linux stub).
pub fn enumerate_printers() -> Vec<PrinterEntry> {
    #[cfg(windows)]
    {
        windows_enumerate_printers()
    }
    #[cfg(not(windows))]
    {
        vec![]
    }
}

/// Construct a `Box<dyn Printer>` for the given `PrinterId`.
///
/// On Windows: routes `Spooler(name)` to `WindowsSpoolerPrinter` and
/// `Serial(port)` to `SerialPrinter` — **Plan 02 implements** the Windows impls.
/// On Linux: returns a `StubPrinter` for any id.
pub fn printer_from_entry(id: &PrinterId) -> Box<dyn Printer> {
    #[cfg(windows)]
    {
        match id {
            PrinterId::Spooler(name) => {
                Box::new(spooler::WindowsSpoolerPrinter::new(name.clone()))
            }
            PrinterId::Serial(port) => {
                Box::new(serial::SerialPrinter::new(port.clone()))
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = id; // suppress unused-variable warning
        Box::new(stub::StubPrinter)
    }
}

// ── Windows-only enumeration ─────────────────────────────────────────────────

/// Enumerate Windows printers (spooler + COM ports) into a combined list.
///
/// Called only on Windows. Plan 02 implements the Windows-specific body.
/// Left as `todo!()` stubs annotated `// Plan 02 implements` per the plan spec.
#[cfg(windows)]
fn windows_enumerate_printers() -> Vec<PrinterEntry> {
    use printers::{get_default_printer, get_printers};
    use serialport::available_ports;

    let default_name = get_default_printer().map(|p| p.name.clone());
    let mut entries = Vec::new();

    // Spooler printers (USB path via Win32 spooler)
    for p in get_printers() {
        let is_default = default_name.as_deref() == Some(p.name.as_str());
        entries.push(PrinterEntry {
            display_name: format!("{} (USB)", p.name),
            id: PrinterId::Spooler(p.name.clone()),
            is_default,
        });
    }

    // COM port printers (serial path)
    if let Ok(ports) = available_ports() {
        for port in ports {
            entries.push(PrinterEntry {
                display_name: format!("{} (Serial)", port.port_name),
                id: PrinterId::Serial(port.port_name.clone()),
                is_default: false,
            });
        }
    }

    entries
}
