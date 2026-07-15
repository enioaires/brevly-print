//! App directory initialization.
//!
//! Resolves and creates `BrevlyPrint/` inside the platform data directory:
//! - Windows: `%APPDATA%\Roaming\BrevlyPrint\`
//! - Linux:   `$XDG_DATA_HOME/BrevlyPrint/` or `~/.local/share/BrevlyPrint/`

use std::path::PathBuf;

/// Resolve and create (idempotently) the application data directory.
///
/// Must be called at startup **before** opening any database or credential file.
/// `create_dir_all` is a no-op when the directory already exists.
///
/// # Errors
///
/// Returns an `io::Error` if:
/// - The platform data directory cannot be resolved (missing user profile).
/// - Directory creation fails (permissions, disk full, etc.).
pub fn init_app_dir() -> std::io::Result<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Cannot resolve platform data directory — no user profile?",
        )
    })?;
    let app_dir = base.join("BrevlyPrint");
    std::fs::create_dir_all(&app_dir)?;
    Ok(app_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_app_dir_succeeds_and_is_a_dir() {
        let path = init_app_dir().expect("init_app_dir should succeed on this platform");
        assert!(path.exists(), "app dir should exist after init_app_dir");
        assert!(path.is_dir(), "app dir should be a directory");
        assert!(
            path.ends_with("BrevlyPrint"),
            "path should end with BrevlyPrint, got: {path:?}"
        );
    }

    #[test]
    fn test_init_app_dir_is_idempotent() {
        // First call: creates the directory
        let first = init_app_dir().expect("first call should succeed");
        // Second call: directory already exists — must not error
        let second = init_app_dir().expect("second call should succeed (idempotent)");
        assert_eq!(first, second, "both calls should return the same path");
    }
}
