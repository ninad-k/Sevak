//! Windows: send an item to the Recycle Bin with `SHFileOperationW`.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::UI::Shell::{
    SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW,
};

use super::com::ComGuard;
use crate::error::{PlatformError, Result};

/// The double-NUL-terminated list `SHFileOperationW` wants for one path.
/// The shell only accepts backslashes.
fn from_list(path: &Path) -> Vec<u16> {
    let mut wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .map(|unit| {
            if unit == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                unit
            }
        })
        .collect();
    wide.extend([0, 0]);
    wide
}

/// Meaning of the few `DE_*` codes `SHFileOperationW` returns that users meet.
fn describe(code: i32) -> String {
    match code {
        0x71 => "the source and destination are the same file".to_owned(),
        0x74 => "the item is a root folder".to_owned(),
        0x78 => "access denied".to_owned(),
        0x7C => "the path is not valid".to_owned(),
        0x7E | 0x7F => "the item cannot be moved to the Recycle Bin".to_owned(),
        0x80 => "the item is open in another program".to_owned(),
        0x10000 => "the operation failed".to_owned(),
        other => format!("error code 0x{other:X}"),
    }
}

pub(crate) fn move_to_trash(path: &Path) -> Result<()> {
    let _com = ComGuard::new();
    let from = from_list(path);
    // `ALLOWUNDO`: to the Recycle Bin rather than deleted. Without
    // `NOCONFIRMATION` the shell would ask about every item (Sevak asked
    // already), but `WANTNUKEWARNING` keeps the one question that matters:
    // when an item cannot be recycled (a network drive, a file too big for the
    // bin) and would be deleted for good, the shell asks, and "No" aborts.
    let flags = FOF_ALLOWUNDO.0
        | FOF_NOCONFIRMATION.0
        | FOF_WANTNUKEWARNING.0
        | FOF_NOERRORUI.0
        | FOF_SILENT.0;
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: flags as u16,
        ..Default::default()
    };
    // SAFETY: `operation` is fully initialized; `from` is a NUL-NUL-terminated
    // UTF-16 list that outlives the call; no window handle, no destination.
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code != 0 {
        return Err(PlatformError::Os {
            operation: "move to the Recycle Bin",
            message: describe(code),
        });
    }
    if operation.fAnyOperationsAborted.as_bool() {
        return Err(PlatformError::Os {
            operation: "move to the Recycle Bin",
            message: "cancelled".to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_list_uses_backslashes_and_ends_with_two_nuls() {
        let list = from_list(Path::new("C:/Users/me/a b.txt"));
        let text = String::from_utf16(&list[..list.len() - 2]).unwrap();
        assert_eq!(text, r"C:\Users\me\a b.txt");
        assert_eq!(&list[list.len() - 2..], [0, 0]);
    }
}
