//! Windows: encrypt data for the current user with DPAPI (`CryptProtectData`),
//! for the clipboard history and its image files.
//!
//! DPAPI keys are derived from the user's logon credentials, so the data opens
//! only for the same user (on the same machine, or on another machine with the
//! same roaming profile): copying the file to another account or machine, or
//! reading it from a backup, gets ciphertext. Software running as the user can
//! still ask DPAPI to decrypt it; that is outside what this protects against.
//!
//! The call runs with `CRYPTPROTECT_UI_FORBIDDEN`, so it never shows a prompt.

use std::io;

use sevak_core::sealed::Sealer;
use windows::core::PWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

/// Mixed into the encryption so that another program's DPAPI blobs are not
/// accepted as Sevak's (and the other way round). Not a secret.
const ENTROPY: &[u8] = b"sevak clipboard history";

/// The current-user DPAPI [`Sealer`].
#[derive(Debug, Clone, Copy)]
pub struct Dpapi;

fn blob(bytes: &[u8]) -> io::Result<CRYPT_INTEGER_BLOB> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "data is too large"))?,
        // DPAPI only reads its input.
        pbData: bytes.as_ptr().cast_mut(),
    })
}

/// Copies the output blob DPAPI allocated and frees it.
///
/// # Safety
/// `out` must be a blob filled in by a successful DPAPI call.
unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: per the contract, `pbData` points to `cbData` bytes that DPAPI
    // allocated with LocalAlloc; they are copied before being freed.
    unsafe {
        let copy = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        copy
    }
}

impl Sealer for Dpapi {
    fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(plain)?;
        let entropy = blob(ENTROPY)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: the blobs point to live slices for the call; `out` is a valid
        // destination and is taken (copied and freed) only after success.
        unsafe {
            CryptProtectData(
                &input,
                windows::core::w!("Sevak"),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .map_err(|err| io::Error::other(format!("CryptProtectData: {err}")))?;
            Ok(take(out))
        }
    }

    fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(sealed)?;
        let entropy = blob(ENTROPY)?;
        let mut out = CRYPT_INTEGER_BLOB::default();
        let mut description = PWSTR::null();
        // SAFETY: as in `seal`; the description DPAPI allocates is freed below.
        unsafe {
            let result = CryptUnprotectData(
                &input,
                Some(&mut description),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            );
            if !description.is_null() {
                let _ = LocalFree(Some(HLOCAL(description.0.cast())));
            }
            result.map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("CryptUnprotectData: {err}"),
                )
            })?;
            Ok(take(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use sevak_core::sealed::{self, OpenError};

    use super::*;

    // Pure in-memory calls: DPAPI needs no clipboard, window or user interaction.

    #[test]
    fn data_round_trips_and_the_ciphertext_hides_it() {
        let sealed = sealed::seal(&Dpapi, b"clipboard text with a password hunter2").unwrap();
        assert!(!sealed.windows(7).any(|window| window == b"hunter2"));
        let opened = sealed::open(Some(&Dpapi), sealed).unwrap();
        assert_eq!(opened.bytes, b"clipboard text with a password hunter2");
        assert!(opened.was_sealed);
    }

    #[test]
    fn empty_data_round_trips() {
        let sealed = sealed::seal(&Dpapi, b"").unwrap();
        assert_eq!(sealed::open(Some(&Dpapi), sealed).unwrap().bytes, b"");
    }

    #[test]
    fn damaged_or_foreign_data_is_refused_not_guessed() {
        let mut sealed = sealed::seal(&Dpapi, b"data").unwrap();
        let len = sealed.len();
        sealed[len - 1] ^= 0xff;
        assert_eq!(
            sealed::open(Some(&Dpapi), sealed),
            Err(OpenError::Undecryptable)
        );
        let mut foreign = sealed::MAGIC.to_vec();
        foreign.extend_from_slice(b"not a dpapi blob");
        assert_eq!(
            sealed::open(Some(&Dpapi), foreign),
            Err(OpenError::Undecryptable)
        );
    }

    #[test]
    fn a_large_image_sized_blob_round_trips() {
        let data: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let sealed = sealed::seal(&Dpapi, &data).unwrap();
        assert_eq!(sealed::open(Some(&Dpapi), sealed).unwrap().bytes, data);
    }
}
