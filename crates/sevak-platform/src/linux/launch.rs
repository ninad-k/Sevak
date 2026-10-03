//! Starting applications.

use std::ffi::OsStr;
use std::path::Path;

use sevak_core::LaunchTarget;

use crate::error::{PlatformError, Result};
use crate::process::{find_in_path, spawn_detached_in};

/// Terminal emulators tried in order for `Terminal=true` entries when `gio`
/// is not available, with the flag that introduces the command to run.
const TERMINALS: [(&str, &str); 5] = [
    ("x-terminal-emulator", "-e"),
    ("ptyxis", "--"),
    ("gnome-terminal", "--"),
    ("konsole", "-e"),
    ("xterm", "-e"),
];

pub(super) fn launch(target: &LaunchTarget) -> Result<()> {
    match target {
        LaunchTarget::DesktopEntry {
            desktop_id,
            path,
            exec,
            terminal,
            working_dir,
        } => {
            tracing::debug!(%desktop_id, "launching desktop entry");
            launch_desktop_entry(path, exec, *terminal, working_dir.as_deref())
        }
        LaunchTarget::Executable {
            path,
            args,
            working_dir,
        } => {
            let program = path.to_str().ok_or_else(|| PlatformError::Os {
                operation: "launch",
                message: format!("executable path is not valid UTF-8: {}", path.display()),
            })?;
            spawn_detached_in(program, args, working_dir.as_deref())
        }
        LaunchTarget::Shortcut { .. } => Err(PlatformError::Unsupported("Windows shortcuts")),
        LaunchTarget::PackagedApp { .. } => Err(PlatformError::Unsupported("packaged apps")),
    }
}

fn launch_desktop_entry(
    path: &Path,
    exec: &[String],
    terminal: bool,
    working_dir: Option<&Path>,
) -> Result<()> {
    // `gio launch` (GLib 2.67+) does everything the spec asks of a launcher:
    // Terminal=true, DBusActivatable, startup notification, Path=, field codes.
    if path.is_file() && find_in_path("gio").is_some() {
        let args: [&OsStr; 2] = [OsStr::new("launch"), path.as_os_str()];
        return spawn_detached_in("gio", &args, None);
    }

    let argv = if terminal {
        terminal_command(exec, |program| find_in_path(program).is_some()).ok_or_else(|| {
            PlatformError::Os {
                operation: "launch",
                message: "no terminal emulator found for a Terminal=true application".into(),
            }
        })?
    } else {
        exec.to_vec()
    };
    let (program, args) = argv.split_first().ok_or_else(|| PlatformError::Os {
        operation: "launch",
        message: "the desktop entry has no Exec command".into(),
    })?;
    spawn_detached_in(program, args, working_dir.filter(|dir| dir.is_dir()))
}

/// Wraps `exec` in the first available terminal emulator.
fn terminal_command(exec: &[String], available: impl Fn(&str) -> bool) -> Option<Vec<String>> {
    if exec.is_empty() {
        return None;
    }
    let (terminal, flag) = TERMINALS.iter().find(|(name, _)| available(name))?;
    let mut argv = vec![(*terminal).to_owned(), (*flag).to_owned()];
    argv.extend_from_slice(exec);
    Some(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec() -> Vec<String> {
        vec!["htop".to_owned(), "-d".to_owned(), "5".to_owned()]
    }

    #[test]
    fn terminal_wrapping_uses_first_available() {
        let argv = terminal_command(&exec(), |p| p == "gnome-terminal" || p == "xterm").unwrap();
        assert_eq!(argv, ["gnome-terminal", "--", "htop", "-d", "5"]);
        let argv = terminal_command(&exec(), |p| p == "konsole").unwrap();
        assert_eq!(argv, ["konsole", "-e", "htop", "-d", "5"]);
        let argv = terminal_command(&exec(), |_| true).unwrap();
        assert_eq!(argv[..2], ["x-terminal-emulator", "-e"]);
        let argv = terminal_command(&exec(), |p| p == "ptyxis").unwrap();
        assert_eq!(argv[..2], ["ptyxis", "--"]);
    }

    #[test]
    fn no_terminal_or_no_command() {
        assert_eq!(terminal_command(&exec(), |_| false), None);
        assert_eq!(terminal_command(&[], |_| true), None);
    }

    #[test]
    fn windows_targets_are_unsupported() {
        let result = launch(&LaunchTarget::PackagedApp {
            app_user_model_id: "x".into(),
        });
        assert!(matches!(result, Err(PlatformError::Unsupported(_))));
    }

    #[test]
    fn fallback_spawns_the_exec_in_the_working_directory() {
        let dir = tempfile::tempdir().unwrap();
        // A desktop file that does not exist forces the direct-spawn path
        // even where `gio` is installed.
        let result = launch(&LaunchTarget::DesktopEntry {
            desktop_id: "test.desktop".into(),
            path: dir.path().join("missing.desktop"),
            exec: vec!["sh".into(), "-c".into(), "pwd > marker".into()],
            terminal: false,
            working_dir: Some(dir.path().to_path_buf()),
        });
        result.unwrap();

        let marker = dir.path().join("marker");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let written = std::fs::read_to_string(&marker).unwrap();
        let expected = dir.path().canonicalize().unwrap();
        assert_eq!(std::path::Path::new(written.trim()), expected);
    }

    #[test]
    fn executable_targets_are_spawned() {
        let dir = tempfile::tempdir().unwrap();
        launch(&LaunchTarget::Executable {
            path: "sh".into(),
            args: vec!["-c".into(), "echo hi > out".into()],
            working_dir: Some(dir.path().to_path_buf()),
        })
        .unwrap();
        let out = dir.path().join("out");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !out.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(out.exists());
    }
}
