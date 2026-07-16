// src/update/verify.rs — portable. No #[cfg(windows)] anywhere in this file.
// Must compile and pass `cargo test --lib` on Linux (D-07).
//
// `sha2 = "0.11.0"` must be listed in [dependencies] in Cargo.toml (not just
// the Windows-gated section) so this portable module can use it on Linux.

use sha2::{Digest, Sha256};

/// Verify that `bytes` SHA-256 hashes to `expected_hex` (case-insensitive hex).
///
/// Returns `Ok(())` on match.
/// Returns `Err` on mismatch, or if `expected_hex` is not exactly 64 hex chars.
///
/// Pure function — no I/O, no Windows deps — Linux unit-testable (D-02 / D-07).
pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> anyhow::Result<()> {
    let hash = Sha256::digest(bytes);
    // Hex-encode without an extra crate (`hex` is not in Cargo.toml):
    let computed: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    if computed.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        anyhow::bail!("SHA256 mismatch: expected {expected_hex}, got {computed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn correct_bytes_and_hex_ok() {
        let data = b"hello world";
        let hex = sha256_hex(data);
        assert!(verify_sha256(data, &hex).is_ok());
    }

    #[test]
    fn tampered_bytes_returns_err() {
        let data = b"hello world";
        let hex = sha256_hex(data);
        let mut tampered = data.to_vec();
        tampered[0] ^= 0xFF;
        assert!(verify_sha256(&tampered, &hex).is_err());
    }

    #[test]
    fn uppercase_expected_hex_accepted() {
        let data = b"hello world";
        let hex = sha256_hex(data).to_uppercase();
        assert!(verify_sha256(data, &hex).is_ok());
    }

    #[test]
    fn wrong_length_hex_returns_err() {
        // 3 hex chars, not 64 — should fail (won't match the 64-char computed hash)
        assert!(verify_sha256(b"data", "abc").is_err());
    }

    #[test]
    fn empty_bytes_with_correct_hash_ok() {
        let data: &[u8] = &[];
        let hex = sha256_hex(data);
        assert!(verify_sha256(data, &hex).is_ok());
    }

    #[test]
    fn empty_bytes_with_nonempty_hash_err() {
        let data: &[u8] = &[];
        let nonempty_hex = sha256_hex(b"nonempty");
        assert!(verify_sha256(data, &nonempty_hex).is_err());
    }
}
