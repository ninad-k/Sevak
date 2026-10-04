//! Opening files, folders and URLs with the system default handler.

use std::ffi::OsStr;
use std::path::Path;

use crate::deep_link::DeepLink;
use crate::error::{PlatformError, Result};

/// Opens a file or folder with the default handler.
pub fn open_path(path: &Path) -> Result<()> {
    open_target(path.as_os_str())
}

/// Shows a file or folder selected in the system file manager: Explorer's
/// `/select`, Finder's `open -R`, or the freedesktop `FileManager1` service
/// (falling back to opening the parent folder on Linux).
pub fn reveal_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(PlatformError::Os {
            operation: "reveal",
            message: format!("{} no longer exists", path.display()),
        });
    }
    reveal_in_file_manager(path)
}

/// Opens a web or mail link in the default browser/mail client.
///
/// Only `http://`, `https://` and `mailto:` are accepted: URLs reach us from
/// user-defined commands and search results, and other schemes (`file:`,
/// `ms-msdt:`, ...) could run arbitrary handlers. The address is parsed and
/// checked by [`sevak_core::url_check::check_open_url`] (control characters,
/// quotes, a missing host, over-long or odd `mailto:` links are refused) and the
/// normalised form is what the handler receives.
pub fn open_url(url: &str) -> Result<()> {
    let url = sevak_core::url_check::check_open_url(url).map_err(invalid_url)?;
    open_target(OsStr::new(&url))
}

/// Opens a link from the closed [`DeepLink`] list (`tel:`, 1Password and macOS
/// Contacts links). This is deliberately not [`open_url`]: that function stays
/// limited to web and mail links, and a `DeepLink` can only be built from the
/// validated pieces its constructors accept.
pub fn open_deep_link(link: &DeepLink) -> Result<()> {
    // The constructors already limit the alphabet; this is the last look.
    let unsafe_char = |c: char| c.is_control() || matches!(c, '"' | '<' | '>' | '\\' | ' ');
    if link.as_str().chars().any(unsafe_char) {
        return Err(invalid_url(
            "the link contains a character that is not allowed",
        ));
    }
    open_target(OsStr::new(link.as_str()))
}

/// Opens a text file (such as `config.toml`) for editing.
pub fn open_in_editor(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use crate::process::spawn_detached;
        use crate::windows::shell_execute;

        match shell_execute("open", path.as_os_str(), None) {
            Ok(()) => {
                tracing::debug!("opened in editor via ShellExecuteW");
                Ok(())
            }
            Err(err) if err.is_no_association() => {
                // Nothing is registered for e.g. `.toml`; Notepad always exists.
                tracing::debug!("no association, falling back to notepad");
                spawn_detached("notepad.exe", &[path])
            }
            Err(err) => Err(err.into()),
        }
    }
    #[cfg(target_os = "macos")]
    {
        // `-t`: the default text editor, whatever `.toml` is associated with.
        let args: [&OsStr; 2] = [OsStr::new("-t"), path.as_os_str()];
        crate::process::spawn_detached(crate::macos::OPEN, &args)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // shared-mime-info maps `.toml` to text/plain, so the default handler
        // is a text editor.
        open_path(path)
    }
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
    tracing::debug!("opened via ShellExecuteW");
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_target(target: &OsStr) -> Result<()> {
    crate::process::spawn_detached(crate::macos::OPEN, &[target])?;
    tracing::debug!("opened via open(1)");
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
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
    tracing::debug!(opener, "opened via external opener");
    Ok(())
}

#[cfg(windows)]
fn reveal_in_file_manager(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    // Explorer parses its own command line: `/select,"<path>"` must reach it
    // verbatim, which `Command::arg` would re-quote.
    Command::new("explorer.exe")
        .raw_arg(explorer_select_arg(path))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|err| PlatformError::CommandFailed {
            command: "explorer.exe".to_owned(),
            message: err.to_string(),
        })?;
    tracing::debug!("revealed via explorer /select");
    Ok(())
}

/// `/select,"C:\dir\file"`. Explorer wants backslashes and no `\\?\` prefix.
#[cfg(windows)]
fn explorer_select_arg(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    format!("/select,\"{text}\"")
}

#[cfg(target_os = "macos")]
fn reveal_in_file_manager(path: &Path) -> Result<()> {
    let args: [&OsStr; 2] = [OsStr::new("-R"), path.as_os_str()];
    crate::process::spawn_detached(crate::macos::OPEN, &args)?;
    tracing::debug!("revealed via open -R");
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn reveal_in_file_manager(path: &Path) -> Result<()> {
    if show_items_over_dbus(path) {
        tracing::debug!("revealed via FileManager1.ShowItems");
        return Ok(());
    }
    // No file manager speaks the interface (or no D-Bus session): open the
    // folder instead. Nothing is highlighted, but the user lands next to it.
    let folder = path.parent().filter(|p| !p.as_os_str().is_empty());
    tracing::debug!("ShowItems unavailable, opening the parent folder");
    open_path(folder.unwrap_or(path))
}

/// Asks the desktop's file manager to select `path` through
/// `org.freedesktop.FileManager1.ShowItems`. Returns whether it answered.
#[cfg(not(any(windows, target_os = "macos")))]
fn show_items_over_dbus(path: &Path) -> bool {
    use std::process::{Command, Stdio};

    use crate::process::find_in_path;

    if find_in_path("gdbus").is_none() {
        return false;
    }
    let uris = format!("['{}']", file_uri(path));
    // `--timeout` bounds the wait for a file manager that has to start first.
    Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--timeout",
            "3",
            "--dest",
            "org.freedesktop.FileManager1",
            "--object-path",
            "/org/freedesktop/FileManager1",
            "--method",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(uris)
        // The startup id, as an (empty) GVariant string.
        .arg("''")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// A `file://` URI for an absolute path, percent-encoding everything except
/// unreserved characters and `/`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn file_uri(path: &Path) -> String {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = path.to_string_lossy().replace('\\', "/").into_bytes();

    let mut uri = String::from("file://");
    for byte in bytes {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(char::from(byte));
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_are_percent_encoded() {
        assert_eq!(
            file_uri(Path::new("/home/a b/it's#1.txt")),
            "file:///home/a%20b/it%27s%231.txt"
        );
        assert_eq!(file_uri(Path::new("/tmp/é")), "file:///tmp/%C3%A9");
    }

    #[test]
    fn revealing_a_missing_path_is_an_error() {
        let missing = std::env::temp_dir().join("sevak-definitely-not-here-0f3a");
        let err = reveal_path(&missing).unwrap_err();
        assert!(err.to_string().contains("no longer exists"), "{err}");
    }

    #[cfg(windows)]
    #[test]
    fn explorer_argument_selects_the_item() {
        assert_eq!(
            explorer_select_arg(Path::new("C:/Users/a b/c.txt")),
            r#"/select,"C:\Users\a b\c.txt""#
        );
        assert_eq!(
            explorer_select_arg(Path::new(r"\\?\C:\x\y")),
            r#"/select,"C:\x\y""#
        );
    }

    #[test]
    fn rejects_empty_and_unsupported_urls() {
        assert!(open_url("   ").is_err());
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
        assert!(open_url("example.com").is_err());
    }

    /// Everything here must fail before the operating system is asked to open
    /// anything (a valid link would start the browser).
    #[test]
    fn adversarial_urls_never_reach_the_handler() {
        for url in [
            "https://example.com/\" --flag",
            "https://example.com/a\nb",
            "https://example.com/a\0b",
            "https://example.com/<x>",
            "https://example.com\\evil",
            "https://",
            "https://user:pw@example.com/",
            "mailto:a@example.com?attach=C:/x",
            "ms-msdt:/id x",
            "HTTPS:example.com",
        ] {
            let err = open_url(url).unwrap_err();
            assert!(err.to_string().contains("open_url"), "{url:?}: {err}");
        }
        let too_long = format!("https://example.com/{}", "a".repeat(9000));
        assert!(open_url(&too_long).is_err());
    }
}
