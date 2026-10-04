//! The Windows version, from the registry (what `winver` shows).

use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE,
};

const KEY: PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");

/// The raw registry values.
pub(crate) struct RawVersion {
    /// `Windows 10 Pro` (it says 10 on Windows 11 too).
    pub(crate) product_name: String,
    /// `24H2`; `ReleaseId` where `DisplayVersion` does not exist.
    pub(crate) display_version: String,
    /// `26100`.
    pub(crate) build: String,
    /// The update revision (`UBR`).
    pub(crate) revision: Option<u32>,
}

pub(crate) fn read() -> Option<RawVersion> {
    let mut key = HKEY::default();
    // SAFETY: the key name is a static NUL-terminated string and `key` is a
    // valid out pointer; the handle is closed below.
    unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, KEY, None, KEY_QUERY_VALUE, &mut key) }
        .ok()
        .ok()?;
    let version = RawVersion {
        product_name: read_string(key, w!("ProductName")).unwrap_or_default(),
        display_version: read_string(key, w!("DisplayVersion"))
            .or_else(|| read_string(key, w!("ReleaseId")))
            .unwrap_or_default(),
        build: read_string(key, w!("CurrentBuildNumber")).unwrap_or_default(),
        revision: read_dword(key, w!("UBR")),
    };
    // SAFETY: closes the handle opened above.
    let _ = unsafe { RegCloseKey(key) };
    Some(version)
}

fn read_dword(key: HKEY, name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: `value` is valid for `size` bytes and both outlive the call.
    let status = unsafe {
        RegQueryValueExW(
            key,
            name,
            None,
            None,
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(value)
}

fn read_string(key: HKEY, name: PCWSTR) -> Option<String> {
    let mut size = 0u32;
    // SAFETY: only the size is asked for; `size` outlives the call.
    let status = unsafe { RegQueryValueExW(key, name, None, None, None, Some(&mut size)) };
    if status.is_err() || size == 0 {
        return None;
    }
    // One extra unit so the text is always terminated.
    let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: `buffer` is valid for `size` bytes and both outlive the call.
    let status = unsafe {
        RegQueryValueExW(
            key,
            name,
            None,
            None,
            Some(buffer.as_mut_ptr().cast::<u8>()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let units = (size as usize / 2).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..units]);
    let text = text.trim_end_matches('\0').trim().to_owned();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_version_can_be_read() {
        let version = super::read().expect("the registry has the Windows version");
        assert!(!version.product_name.is_empty());
        assert!(version.build.parse::<u32>().is_ok(), "{}", version.build);
    }
}
