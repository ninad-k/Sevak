//! Protecting a small secret (an API key) before it is written to disk.
//!
//! | OS | Protection |
//! |---|---|
//! | Windows | DPAPI (`CryptProtectData`, current user): the bytes can only be read back by the same Windows user on the same machine |
//! | macOS, Linux | none of its own: the caller writes the bytes with [`crate::private_file::write_atomic`], which makes the file readable by its owner only (mode `0600`) |
//!
//! This is deliberately not a keychain integration: the OS credential stores
//! on macOS (Keychain) and Linux (Secret Service over D-Bus) need dependencies
//! that cannot be tested everywhere. Say plainly which protection applies with
//! [`protection`].

use crate::error::Result;

/// How [`protect`] shields a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    /// Encrypted with the Windows user's DPAPI key.
    Dpapi,
    /// Not encrypted; relies on the file being readable by its owner only.
    OwnerOnlyFile,
}

impl Protection {
    /// The word stored next to the data, so a file written under one
    /// protection is never read as another.
    pub fn id(self) -> &'static str {
        match self {
            Self::Dpapi => "dpapi",
            Self::OwnerOnlyFile => "file",
        }
    }

    /// A plain-language description for the settings window.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Dpapi => "encrypted with Windows DPAPI for your user account",
            Self::OwnerOnlyFile => "in a file only your user account can read (not encrypted)",
        }
    }
}

/// The protection this OS gives [`protect`]ed secrets.
pub fn protection() -> Protection {
    if cfg!(windows) {
        Protection::Dpapi
    } else {
        Protection::OwnerOnlyFile
    }
}

/// Shields `plain` for storage. The result is only meaningful to [`unprotect`]
/// on the same machine and user.
pub fn protect(plain: &[u8]) -> Result<Vec<u8>> {
    #[cfg(windows)]
    {
        crate::windows::secret::protect(plain)
    }
    #[cfg(not(windows))]
    {
        Ok(plain.to_vec())
    }
}

/// Reverses [`protect`]. Fails when the data was protected by another user or
/// machine, or is damaged.
pub fn unprotect(protected: &[u8]) -> Result<Vec<u8>> {
    #[cfg(windows)]
    {
        crate::windows::secret::unprotect(protected)
    }
    #[cfg(not(windows))]
    {
        Ok(protected.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_round_trips() {
        let secret = b"sk-test-0123456789";
        let stored = protect(secret).unwrap();
        assert_eq!(unprotect(&stored).unwrap(), secret);
    }

    #[test]
    fn empty_and_binary_secrets_round_trip() {
        for secret in [&b""[..], &[0u8, 255, 10, 13][..]] {
            assert_eq!(unprotect(&protect(secret).unwrap()).unwrap(), secret);
        }
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_output_does_not_contain_the_secret() {
        let secret = b"sk-test-0123456789";
        let stored = protect(secret).unwrap();
        assert!(!stored.windows(secret.len()).any(|w| w == secret));
        assert_eq!(protection(), Protection::Dpapi);
    }

    #[cfg(windows)]
    #[test]
    fn damaged_dpapi_data_is_refused() {
        assert!(unprotect(b"definitely not a dpapi blob").is_err());
    }

    #[cfg(not(windows))]
    #[test]
    fn elsewhere_the_file_is_the_protection() {
        assert_eq!(protection(), Protection::OwnerOnlyFile);
    }
}
