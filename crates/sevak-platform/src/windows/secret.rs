//! Windows: DPAPI for small secrets (see [`crate::secret`]).

use std::ffi::c_void;

use windows::core::w;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use crate::error::{PlatformError, Result};

/// Mixed into the encryption, so another program that merely calls
/// `CryptUnprotectData` as the same user does not get the key by accident.
const ENTROPY: &[u8] = b"sevak-secret-v1";

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    }
}

/// Copies a blob DPAPI allocated and frees it.
///
/// # Safety
/// `out` must come from a successful `CryptProtectData` / `CryptUnprotectData`.
unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
    let _ = LocalFree(Some(HLOCAL(out.pbData.cast::<c_void>())));
    bytes
}

pub(crate) fn protect(plain: &[u8]) -> Result<Vec<u8>> {
    let input = blob(plain);
    let entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: the blobs point at slices that outlive the call; `out` is
    // filled by DPAPI and released by `take`.
    unsafe {
        CryptProtectData(
            &input,
            w!("Sevak"),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
        .map_err(|err| PlatformError::Os {
            operation: "CryptProtectData",
            message: err.message(),
        })?;
        Ok(take(out))
    }
}

pub(crate) fn unprotect(protected: &[u8]) -> Result<Vec<u8>> {
    let input = blob(protected);
    let entropy = blob(ENTROPY);
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as in `protect`.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
        .map_err(|err| PlatformError::Os {
            operation: "CryptUnprotectData",
            message: err.message(),
        })?;
        Ok(take(out))
    }
}
