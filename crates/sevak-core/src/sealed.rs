//! Files that are encrypted for the current user (the clipboard history and the
//! image files beside it).
//!
//! The encryption itself is the operating system's (on Windows DPAPI with the
//! current-user scope: only the same user on the same machine, or a roaming
//! profile of the same user, can read the data back), so it lives behind the
//! [`Sealer`] trait and `sevak-platform` provides the implementation. This
//! module has the file format and the rules around it:
//!
//! - A sealed file is [`MAGIC`] followed by whatever the sealer produced. A file
//!   without the magic is plain data, so older files and systems without a
//!   sealer keep working; [`open`] says which of the two it was.
//! - A sealed file that cannot be opened (another user, another machine, a lost
//!   key, damaged bytes, or no sealer on this system) is an [`OpenError`], never
//!   a guess: the caller starts afresh and says so.
//!
//! Readers that are not part of the history code, such as the preview pane and
//! the icon loader, find the sealer through [`global`], which the platform
//! crate fills in when it starts.
//!
//! This is protection against someone who gets the *file* (a backup, a roaming
//! profile, another account). Software running as the same user can ask the OS
//! to decrypt it just as Sevak does.

use std::io;
use std::sync::{Arc, RwLock};

/// What every sealed file starts with.
pub const MAGIC: &[u8] = b"SEVAK-SEALED-1\n";

/// Encrypts and decrypts for the current user.
pub trait Sealer: Send + Sync {
    /// Encrypts `plain`.
    fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>>;
    /// Decrypts what [`Sealer::seal`] made. An error means this user cannot
    /// read it.
    fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>>;
}

/// Why a sealed file could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// This system has no way to open sealed files.
    NoSealer,
    /// The sealer refused: written for another user or machine, or damaged.
    Undecryptable,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoSealer => "this system cannot open encrypted files",
            Self::Undecryptable => "the file is encrypted for another user or is damaged",
        })
    }
}

impl std::error::Error for OpenError {}

/// True if `bytes` start like a sealed file.
pub fn is_sealed(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// `plain`, encrypted, with the magic in front.
pub fn seal(sealer: &dyn Sealer, plain: &[u8]) -> io::Result<Vec<u8>> {
    let body = sealer.seal(plain)?;
    let mut out = Vec::with_capacity(MAGIC.len() + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&body);
    Ok(out)
}

/// What [`open`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// The file's contents, decrypted if they were sealed.
    pub bytes: Vec<u8>,
    /// Whether the file was sealed (false: it was plain data).
    pub was_sealed: bool,
}

/// The contents of a file read from disk: decrypted when sealed, as they are
/// when not.
pub fn open(sealer: Option<&dyn Sealer>, bytes: Vec<u8>) -> Result<Opened, OpenError> {
    if !is_sealed(&bytes) {
        return Ok(Opened {
            bytes,
            was_sealed: false,
        });
    }
    let sealer = sealer.ok_or(OpenError::NoSealer)?;
    let plain = sealer
        .unseal(&bytes[MAGIC.len()..])
        .map_err(|_| OpenError::Undecryptable)?;
    Ok(Opened {
        bytes: plain,
        was_sealed: true,
    })
}

static GLOBAL: RwLock<Option<Arc<dyn Sealer>>> = RwLock::new(None);

/// Sets the sealer that readers without one of their own use (see the module
/// documentation). `None` removes it.
pub fn install_global(sealer: Option<Arc<dyn Sealer>>) {
    *GLOBAL
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = sealer;
}

/// The installed sealer, if any.
pub fn global() -> Option<Arc<dyn Sealer>> {
    GLOBAL
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// [`open`] with the [`global`] sealer, for readers of image files that may be
/// sealed (the preview pane, icons).
pub fn open_global(bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let sealer = global();
    open(sealer.as_deref(), bytes)
        .map(|opened| opened.bytes)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

/// A sealer for tests: XORs with a key byte behind a marker, so the same key
/// opens it and any other key (another "user") does not.
#[cfg(any(test, feature = "test-util"))]
pub mod fake {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    pub struct XorSealer(pub u8);

    const MARKER: &[u8] = b"XOR:";

    impl Sealer for XorSealer {
        fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
            let mut out = MARKER.to_vec();
            out.push(self.0);
            out.extend(plain.iter().map(|byte| byte ^ self.0));
            Ok(out)
        }

        fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
            let body = sealed
                .strip_prefix(MARKER)
                .and_then(|rest| rest.split_first())
                .filter(|(key, _)| **key == self.0)
                .map(|(_, body)| body)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "wrong key"))?;
            Ok(body.iter().map(|byte| byte ^ self.0).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::XorSealer;
    use super::*;

    #[test]
    fn sealed_data_round_trips_and_is_not_plain() {
        let sealer = XorSealer(0x5a);
        let sealed = seal(&sealer, b"top secret clipboard text").unwrap();
        assert!(is_sealed(&sealed));
        assert!(!sealed
            .windows(b"secret".len())
            .any(|window| window == b"secret"));
        let opened = open(Some(&sealer), sealed).unwrap();
        assert_eq!(opened.bytes, b"top secret clipboard text");
        assert!(opened.was_sealed);
    }

    #[test]
    fn plain_data_passes_through_and_says_so() {
        let opened = open(Some(&XorSealer(1)), b"{\"version\":2}".to_vec()).unwrap();
        assert_eq!(opened.bytes, b"{\"version\":2}");
        assert!(!opened.was_sealed);
        // Even where there is no sealer.
        assert!(open(None, b"plain".to_vec()).is_ok());
    }

    #[test]
    fn another_user_or_no_sealer_cannot_open_it() {
        let sealed = seal(&XorSealer(1), b"data").unwrap();
        assert_eq!(
            open(Some(&XorSealer(2)), sealed.clone()),
            Err(OpenError::Undecryptable)
        );
        assert_eq!(open(None, sealed), Err(OpenError::NoSealer));
    }

    #[test]
    fn damaged_bytes_are_undecryptable() {
        let mut sealed = seal(&XorSealer(1), b"data").unwrap();
        sealed.truncate(MAGIC.len() + 2);
        assert_eq!(
            open(Some(&XorSealer(1)), sealed),
            Err(OpenError::Undecryptable)
        );
        // The magic and nothing else.
        assert_eq!(
            open(Some(&XorSealer(1)), MAGIC.to_vec()),
            Err(OpenError::Undecryptable)
        );
    }

    #[test]
    fn the_global_sealer_serves_readers_without_one() {
        // The only test that touches the global.
        install_global(Some(Arc::new(XorSealer(7))));
        let sealed = seal(&XorSealer(7), b"image bytes").unwrap();
        assert_eq!(open_global(sealed).unwrap(), b"image bytes");
        assert_eq!(open_global(b"png".to_vec()).unwrap(), b"png");
        install_global(None);
        let sealed = seal(&XorSealer(7), b"image bytes").unwrap();
        assert!(open_global(sealed).is_err());
    }
}
