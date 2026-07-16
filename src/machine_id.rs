//! Machine ID reader — stable hardware identifier for the Noren activate request.
//!
//! On Windows: reads `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` via `winreg`.
//! On Linux/non-Windows: returns `None` (machineId is omitted from the activate request).
//!
//! MachineGuid is generated at Windows installation time and survives hardware changes;
//! it changes only on OS reinstall — exactly the right granularity for the re-bind use
//! case (CONTEXT D-02).
//!
//! Mirrors the dual-cfg factory function pattern from `src/credential_store/mod.rs`.

/// Return the machine's stable hardware identifier, if available.
///
/// - **Windows:** reads `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` (string value).
///   Returns `None` if the registry key is missing or unreadable.
/// - **Non-Windows:** always returns `None` — `machineId` is omitted from the activate
///   request body via `#[serde(skip_serializing_if = "Option::is_none")]`.
///
/// The `machineId` is not a secret (it is sent in the activate request body over HTTPS).
/// It disambiguates physical machines for the 409 re-bind flow; do not log it unnecessarily.
#[cfg(windows)]
pub fn get_machine_id() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey("SOFTWARE\\Microsoft\\Cryptography")
        .ok()?;
    key.get_value("MachineGuid").ok()
}

#[cfg(not(windows))]
pub fn get_machine_id() -> Option<String> {
    None // Linux dev: omit machineId from activate request
}
