//! SHA-256 checks for downloads: the theme gallery and the workflow gallery
//! both list a file's hash in their index, and nothing is installed unless the
//! downloaded bytes match it. The download itself is
//! `sevak_plugins::net::fetch_https`.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

/// The lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Checks that `bytes` hash to `expected` (64 hex digits, any case, surrounding
/// whitespace ignored). The error says the download was discarded; callers may
/// replace it with their own wording.
pub fn verify_sha256(bytes: &[u8], expected: &str) -> Result<(), String> {
    if sha256_hex(bytes).eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err(
            "the download does not match the checksum in the gallery index, so it was \
             discarded"
                .to_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums() {
        // The well-known SHA-256 values of "" and "abc".
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(verify_sha256(b"abc", &sha256_hex(b"abc")).is_ok());
        assert!(verify_sha256(b"abc", &format!(" {} ", sha256_hex(b"abc").to_uppercase())).is_ok());
        let err = verify_sha256(b"abd", &sha256_hex(b"abc")).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(verify_sha256(b"", "").is_err());
    }
}
