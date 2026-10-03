//! Opening files, folders and URLs with the system default handler.

use std::ffi::OsStr;
use std::path::Path;

use crate::error::{PlatformError, Result};

/// Opens a file or folder with the default handler.
pub fn open_path(path: &Path) -> Result<()> {
    open_target(path.as_os_str())
}

/// Opens a web or mail link in the default browser/mail client.
///
/// Only `http://`, `https://` and `mailto:` are accepted: URLs reach us from
/// user-defined commands and search results, and other schemes (`file:`,
/// `ms-msdt:`, ...) could run arbitrary handlers.
pub fn open_url(url: &str) -> Result<()> {
    let url = url.trim();
    if url.is_empty() {
        return Err(invalid_url("the URL is empty"));
    }
    if !has_allowed_scheme(url) {
        return Err(invalid_url(
            "only http://, https:// and mailto: links can be opened",
        ));
    }
    open_target(OsStr::new(url))
}

/// Opens a text file (such as `config.toml`) for editing.
pub fn open_in_editor(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use crate::process::spawn_detached;
        use crate::windows::shell_execute;

        match shell_execute("open", path.as_os_str(), None) {
            Ok(()) => {
                tracing::debug!(path = %path.display(), "opened in editor via ShellExecuteW");
                Ok(())
            }
            Err(err) if err.is_no_association() => {
                // Nothing is registered for e.g. `.toml`; Notepad always exists.
                tracing::debug!(path = %path.display(), "no association, falling back to notepad");
                spawn_detached("notepad.exe", &[path])
            }
            Err(err) => Err(err.into()),
        }
    }
    #[cfg(not(windows))]
    {
        // shared-mime-info maps `.toml` to text/plain, so the default handler
        // is a text editor.
        open_path(path)
    }
}

fn has_allowed_scheme(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

fn invalid_url(message: &str) -> PlatformError {
    PlatformError::Os {
        operation: "open_url",
        message: message.to_owned(),
    }
}

#[cfg(windows)]
fn open_target(target: &OsStr) -> Result<()> {
    crate::windows::shell_execute("open", target, None)?;
    tracing::debug!(target = %target.to_string_lossy(), "opened via ShellExecuteW");
    Ok(())
}

#[cfg(not(windows))]
fn open_target(target: &OsStr) -> Result<()> {
    use crate::process::{find_in_path, spawn_detached};

    // `gio open` honours the GNOME/GVfs handler setup; xdg-open is the portable
    // fallback.
    let (opener, args): (&str, Vec<&OsStr>) = if find_in_path("gio").is_some() {
        ("gio", vec![OsStr::new("open"), target])
    } else if find_in_path("xdg-open").is_some() {
        ("xdg-open", vec![target])
    } else {
        return Err(PlatformError::CommandFailed {
            command: "xdg-open".to_owned(),
            message: "neither `gio` nor `xdg-open` was found on PATH; install xdg-utils".to_owned(),
        });
    };

    spawn_detached(opener, &args)?;
    tracing::debug!(opener, target = %target.to_string_lossy(), "opened via external opener");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_unsupported_urls() {
        assert!(open_url("   ").is_err());
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
        assert!(open_url("example.com").is_err());
    }

    #[test]
    fn scheme_check_is_case_insensitive() {
        assert!(has_allowed_scheme("HTTPS://example.com"));
        assert!(has_allowed_scheme("mailto:a@b.c"));
        assert!(!has_allowed_scheme("ftp://example.com"));
    }
}
