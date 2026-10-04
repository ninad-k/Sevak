//! Telling network and device paths from local ones, by their text.
//!
//! On Windows a path such as `\\server\share\file` makes the system connect to
//! another machine and authenticate to it as soon as anything touches the path
//! (listing it, asking whether it exists, opening it). Sevak reads paths that
//! people type or paste and that other programs hand it, so the text is
//! checked *before* any file system call. These functions only look at the
//! text; `sevak_platform::netpath` adds the operating-system part (mapped
//! network drives) and the per-OS rule.

/// Shown (as a row or an error) when a network path is refused.
pub const NETWORK_PATHS_OFF: &str = "Network paths are turned off (Settings \u{2192} Files)";
/// Shown when a device path is refused.
pub const DEVICE_PATH_REFUSED: &str = "That is a device path, not a file or folder";

/// The text with `/` read as `\` and the NT object prefix `\??\` written as
/// `\\?\`, which is how Windows itself treats them.
fn unify(path: &str) -> String {
    let unified = path.replace('/', "\\");
    match unified.strip_prefix("\\??\\") {
        Some(rest) => format!("\\\\?\\{rest}"),
        None => unified,
    }
}

/// A UNC path: `\\server\share`, `//server/share`, `\\?\UNC\server\share` or
/// `\\.\UNC\server\share`, any case. Opening it makes the system contact
/// another machine (SMB, or WebDAV for `\\host@SSL\share`).
///
/// Local device paths (`\\?\C:\`, `\\.\C:`) are not network paths, see
/// [`is_device_path`] for the others.
pub fn is_network_path(path: &str) -> bool {
    let unified = unify(path);
    let Some(rest) = unified.strip_prefix("\\\\") else {
        return false;
    };
    if rest.starts_with("?\\") || rest.starts_with(".\\") {
        return rest
            .get(2..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("UNC\\"));
    }
    true
}

/// A Windows device path that is neither a network path nor a plain drive:
/// `\\.\pipe\name`, `\\.\COM1`, `\\?\GLOBALROOT\...`, `\\?\Volume{...}\`.
/// `\\?\C:\long\path` is just a local path in long form and is not one.
pub fn is_device_path(path: &str) -> bool {
    let unified = unify(path);
    let Some(rest) = unified.strip_prefix("\\\\") else {
        return false;
    };
    let Some(name) = rest
        .strip_prefix("?\\")
        .or_else(|| rest.strip_prefix(".\\"))
    else {
        return false;
    };
    if is_network_path(path) {
        return false;
    }
    let mut chars = name.chars();
    let drive = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.next() == Some(':')
        && matches!(chars.next(), None | Some('\\'));
    !drive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unc_paths_in_every_spelling_are_network_paths() {
        for path in [
            r"\\server\share",
            r"\\server\share\dir\file.txt",
            r"\\server",
            r"\\",
            "//server/share",
            "//server/share/x",
            r"\\?\UNC\server\share",
            r"\\?\unc\server\share\x",
            r"\\.\UNC\server\share",
            "//?/UNC/server/share",
            r"\??\UNC\server\share",
            r"\\host@SSL\share\x",
            r"\\host@8080\DavWWWRoot\x",
            r"\\localhost\c$\x",
            r"\/server/share",
        ] {
            assert!(is_network_path(path), "{path}");
        }
    }

    #[test]
    fn local_paths_are_not_network_paths() {
        for path in [
            "",
            "/",
            "/etc/hosts",
            "~/x",
            r"C:\Users\me",
            "C:/Users/me",
            r"\\?\C:\Users\me",
            r"\\.\C:",
            r"\\.\pipe\name",
            r"\\?\Volume{1234}\x",
            r"\Users\me",
            "relative/dir",
            "a//b",
            "a\\\\b",
        ] {
            assert!(!is_network_path(path), "{path}");
        }
    }

    #[test]
    fn device_paths_are_told_from_drives_and_shares() {
        for path in [
            r"\\.\pipe\name",
            r"\\.\COM1",
            r"\\.\PhysicalDrive0",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1",
            r"\\?\Volume{1234}\x",
            "//./pipe/name",
            r"\??\GLOBALROOT\x",
        ] {
            assert!(is_device_path(path), "{path}");
        }
        for path in [
            r"\\?\C:\Users\me",
            r"\\?\c:",
            r"\\.\C:",
            "//?/D:/x",
            r"\\?\UNC\server\share",
            r"\\server\share",
            r"C:\x",
            "/dev/null",
            "",
        ] {
            assert!(!is_device_path(path), "{path}");
        }
    }
}
