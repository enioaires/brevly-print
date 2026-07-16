//! Windows spooler printer implementation (WritePrinter RAW path).
//!
//! **Windows-only.** This module is compiled only when `cfg(windows)`.
//!
//! Uses the Win32 `OpenPrinterW` / `StartDocPrinterW` / `WritePrinter` sequence to send
//! raw ESC/POS bytes directly to a USB thermal printer via the Windows print spooler.
//!
//! CRITICAL PITFALL C1: `pDatatype` in `DOC_INFO_1W` MUST be set to `"RAW"` or the spooler
//! will interpret ESC/POS bytes as GDI/EMF and produce silent garbage output.
#![cfg(windows)]

use windows::Win32::Graphics::Printing::{
    ClosePrinter, DOC_INFO_1W, EndDocPrinter, EndPagePrinter, OpenPrinterW, PRINTER_HANDLE,
    StartDocPrinterW, StartPagePrinter, WritePrinter,
};
use windows::core::PCWSTR;

use super::{Printer, PrinterError};

/// Windows spooler printer — sends raw ESC/POS bytes via `WritePrinter` with `"RAW"` datatype.
///
/// Created by `printer_from_entry()` for `PrinterId::Spooler(name)`.
pub struct WindowsSpoolerPrinter {
    /// Windows printer name as returned by `printers::get_printers()` → `p.name`.
    /// Must be passed verbatim to `OpenPrinterW` — do NOT transform the name (Pitfall 5).
    pub printer_name: String,
}

impl WindowsSpoolerPrinter {
    pub fn new(printer_name: String) -> Self {
        Self { printer_name }
    }
}

impl Printer for WindowsSpoolerPrinter {
    fn print_raw(&self, bytes: &[u8]) -> Result<(), PrinterError> {
        // SAFETY: Win32 FFI — pointer and length values are correctly derived from owned
        // Vec<u16> and slice references that outlive the unsafe block. No attacker-controlled
        // lengths (T-02-07 accepted, local single-user agent).
        unsafe { write_raw_to_spooler(&self.printer_name, bytes) }
    }
}

/// Win32 WritePrinter RAW sequence — RESEARCH.md Pattern 3.
///
/// Sequence: OpenPrinterW → StartDocPrinterW (DOC_INFO_1W level 1 with "RAW") →
/// StartPagePrinter → WritePrinter → EndPagePrinter → EndDocPrinter → ClosePrinter.
///
/// T-02-06 (handle leak on error path): every early-return after OpenPrinterW
/// calls `ClosePrinter(handle)` to ensure the handle is always released.
unsafe fn write_raw_to_spooler(printer_name: &str, data: &[u8]) -> Result<(), PrinterError> {
    // Encode printer name to UTF-16 null-terminated string.
    // Pitfall 5: pass printer_name verbatim — no transformation.
    let name_w: Vec<u16> = printer_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut handle = PRINTER_HANDLE::default();
    // OpenPrinterW failure with ERROR_INVALID_PRINTER_NAME (1801) → NotFound.
    // SAFETY: name_w is a valid null-terminated UTF-16 buffer that outlives this call.
    unsafe { OpenPrinterW(PCWSTR(name_w.as_ptr()), &mut handle, None) }
        .map_err(|e| PrinterError::NotFound(format!("{printer_name}: {e}")))?;

    // From here on, every exit path must call ClosePrinter (T-02-06).
    let result = unsafe { submit_job(handle, printer_name, data) };

    // Always close the handle — ClosePrinter on a valid handle is infallible in practice.
    unsafe { let _ = ClosePrinter(handle); }

    result
}

/// Inner function that performs the print job while the caller holds the open handle.
/// Separated so that the `ClosePrinter` call in `write_raw_to_spooler` is unconditional.
///
/// WR-01: tracks whether StartDocPrinterW and StartPagePrinter have been called so the
/// matching End* functions are invoked on every error path, preventing zombie spooler jobs.
unsafe fn submit_job(
    handle: PRINTER_HANDLE,
    printer_name: &str,
    data: &[u8],
) -> Result<(), PrinterError> {
    // WR-06: use .chain(std::iter::once(0)) for null termination — consistent with name_w
    // on line 56 and less error-prone than embedding \0 in the string literal.
    let doc_name: Vec<u16> = "BrevlyPrint".encode_utf16().chain(std::iter::once(0)).collect();
    // CRITICAL C1: pDatatype MUST be "RAW" or ESC/POS becomes silent garbage (GDI/EMF).
    // This is the load-bearing detail — omitting "RAW" causes the spooler to interpret
    // ESC/POS bytes as EMF/GDI and produce no visible output. Validated by test-print (D-08).
    let datatype: Vec<u16> = "RAW".encode_utf16().chain(std::iter::once(0)).collect(); // CRITICAL C1: RAW

    let doc_info = DOC_INFO_1W {
        pDocName: windows::core::PWSTR(doc_name.as_ptr() as *mut u16),
        pOutputFile: windows::core::PWSTR::null(),
        // CRITICAL C1: pDatatype = "RAW" — see above.
        pDatatype: windows::core::PWSTR(datatype.as_ptr() as *mut u16),
    };

    // WR-01: state flags so every error path calls the correct End* counterpart.
    let mut doc_started = false;
    let mut page_started = false;

    /// Best-effort cleanup helper — called on all error paths.
    /// Calls EndPagePrinter only if the page was started, then EndDocPrinter only if
    /// the doc was started, matching the WR-01 RAII requirement.
    macro_rules! cleanup_on_err {
        () => {{
            if page_started {
                let _ = EndPagePrinter(handle);
            }
            if doc_started {
                let _ = EndDocPrinter(handle);
            }
        }};
    }

    // StartDocPrinterW: level 1, pointer to DOC_INFO_1W — returns u32 job_id (0 = failure) in windows 0.62.
    let job_id = StartDocPrinterW(handle, 1, &doc_info as *const DOC_INFO_1W);
    if job_id == 0 {
        return Err(PrinterError::PrintFailed(format!(
            "StartDocPrinterW failed for {printer_name}"
        )));
    }
    doc_started = true;

    // StartPagePrinterW must be called before WritePrinter — returns BOOL in windows 0.62.
    if !StartPagePrinter(handle).as_bool() {
        cleanup_on_err!();
        return Err(PrinterError::PrintFailed(format!(
            "StartPagePrinterW failed for {printer_name}"
        )));
    }
    page_started = true;

    let mut bytes_written: u32 = 0;
    // CR-04: guard against silent truncation — usize is 8 bytes on 64-bit; u32 is 4.
    let data_len_u32 = match u32::try_from(data.len()) {
        Ok(n) => n,
        Err(_) => {
            cleanup_on_err!();
            return Err(PrinterError::PrintFailed(format!(
                "Print data too large for WritePrinter ({} bytes > u32::MAX)",
                data.len()
            )));
        }
    };
    // WritePrinter: data pointer + length in bytes. Returns BOOL (inferred — no import needed in 0.62).
    let write_ok = WritePrinter(
        handle,
        data.as_ptr() as *const _,
        data_len_u32,
        &mut bytes_written,
    );
    if !write_ok.as_bool() {
        cleanup_on_err!();
        return Err(PrinterError::PrintFailed(format!(
            "WritePrinter failed for {printer_name}: only {bytes_written}/{} bytes written",
            data.len()
        )));
    }

    // EndPagePrinter / EndDocPrinter return BOOL in windows 0.62 (not Result).
    if !EndPagePrinter(handle).as_bool() {
        return Err(PrinterError::PrintFailed(format!(
            "EndPagePrinterW failed for {printer_name}"
        )));
    }
    if !EndDocPrinter(handle).as_bool() {
        return Err(PrinterError::PrintFailed(format!(
            "EndDocPrinterW failed for {printer_name}"
        )));
    }

    Ok(())
}
