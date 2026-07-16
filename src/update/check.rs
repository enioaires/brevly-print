// src/update/check.rs — portable, no #[cfg(windows)] anywhere in this file.
// Must compile and pass `cargo test --lib` on Linux (D-07).
//
// `semver = "1.0.28"` must be listed in [dependencies] in Cargo.toml (not just
// the Windows-gated section) so this portable module can use it on Linux.

use semver::Version;

/// Decision returned by `check_for_update`.
pub enum UpdateDecision {
    /// The remote version is the same or older — no update needed.
    UpToDate,
    /// The remote version is newer — download and stage the update.
    UpdateAvailable,
    /// Either `current` or `remote` could not be parsed as semver.
    Err(String),
}

/// Compare `current` and `remote` semver strings and decide whether an update is needed.
///
/// Pure: no I/O, no Windows dep. Linux unit-testable (D-07).
///
/// Returns [`UpdateDecision::Err`] if either string is not valid semver.
/// Returns [`UpdateDecision::UpdateAvailable`] if `remote > current`.
/// Returns [`UpdateDecision::UpToDate`] if `remote <= current`.
pub fn check_for_update(current: &str, remote: &str) -> UpdateDecision {
    let cur = match Version::parse(current) {
        Ok(v) => v,
        Err(e) => return UpdateDecision::Err(format!("invalid current version: {e}")),
    };
    let rem = match Version::parse(remote) {
        Ok(v) => v,
        Err(e) => return UpdateDecision::Err(format!("invalid remote version: {e}")),
    };
    if rem > cur {
        UpdateDecision::UpdateAvailable
    } else {
        UpdateDecision::UpToDate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_remote_returns_available() {
        assert!(matches!(
            check_for_update("0.1.0", "0.2.0"),
            UpdateDecision::UpdateAvailable
        ));
    }

    #[test]
    fn older_remote_returns_up_to_date() {
        assert!(matches!(
            check_for_update("0.2.0", "0.1.0"),
            UpdateDecision::UpToDate
        ));
    }

    #[test]
    fn equal_versions_returns_up_to_date() {
        assert!(matches!(
            check_for_update("0.1.0", "0.1.0"),
            UpdateDecision::UpToDate
        ));
    }

    #[test]
    fn malformed_remote_returns_err() {
        assert!(matches!(
            check_for_update("0.1.0", "not-semver"),
            UpdateDecision::Err(_)
        ));
    }

    #[test]
    fn malformed_current_returns_err() {
        assert!(matches!(
            check_for_update("bad", "0.2.0"),
            UpdateDecision::Err(_)
        ));
    }
}
