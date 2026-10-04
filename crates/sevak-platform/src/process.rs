//! Process-level helpers: spawning detached children, console attachment and
//! foreground-window hand-off.

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::error::{PlatformError, Result};

/// Returns the first executable named `program` on `PATH`.
///
/// On Windows a bare name without an extension also matches `<name>.exe`, since
/// users type `notepad` rather than `notepad.exe`.
pub fn find_in_path(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let try_exe = cfg!(windows) && !program.contains('.');
    env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
        if try_exe {
            let with_exe = dir.join(format!("{program}.exe"));
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
        None
    })
}

/// Applies the environment children must start with.
///
/// On Wayland, Sevak sets `GDK_BACKEND=x11` in its own process so that its
/// window runs under XWayland (see [`crate::session::prefer_xwayland`]). If
/// children inherited that, every application launched from Sevak would also be
/// forced onto XWayland (blurry on fractional scaling, no native Wayland
/// features). So when Sevak set the variable itself, it is removed here.
fn apply_child_env(command: &mut Command, xwayland_forced: bool) {
    if xwayland_forced {
        command.env_remove("GDK_BACKEND");
    }
}

/// Variables a script process always inherits from Sevak (compared
/// case-insensitively on Windows, where names have no case). Everything else in
/// Sevak's own environment (tokens, cloud credentials, proxy settings, version
/// manager roots) stays behind unless a plugin's manifest asks for it by name
/// with `inherit_env`.
const BASE_ENV: &[&str] = &[
    "PATH",
    "PATHEXT",
    "HOME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "USER",
    "USERNAME",
    "USERDOMAIN",
    "LOGNAME",
    "SHELL",
    "TEMP",
    "TMP",
    "TMPDIR",
    "TZ",
    "LANG",
    "LANGUAGE",
    "TERM",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "DBUS_SESSION_BUS_ADDRESS",
    "__CF_USER_TEXT_ENCODING",
    // Windows: programs and the C runtime need these to start at all.
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "COMMONPROGRAMFILES",
    "COMMONPROGRAMFILES(X86)",
    "COMMONPROGRAMW6432",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "COMPUTERNAME",
    "OS",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
];

/// Prefixes of the base set: locale and XDG directories.
const BASE_ENV_PREFIXES: &[&str] = &["LC_", "XDG_", "SEVAK_PLUGIN_"];

/// Variables that make an interpreter or loader run something else: startup
/// files, module and library search paths, option strings. Nobody may hand
/// these to a script process, not a workflow and not a manifest's
/// `inherit_env`.
const INTERPRETER_VARS: &[&str] = &[
    "BASH_ENV",
    "ENV",
    "BASHOPTS",
    "SHELLOPTS",
    "PS4",
    "PROMPT_COMMAND",
    "CDPATH",
    "IFS",
    "GLOBIGNORE",
    "ZDOTDIR",
    "PATH",
    "PATHEXT",
    "COMSPEC",
    "PSMODULEPATH",
    "NODE_OPTIONS",
    "NODE_PATH",
    "NODE_EXTRA_CA_CERTS",
    "NODE_V8_COVERAGE",
    "NODE_TLS_REJECT_UNAUTHORIZED",
    "PERLLIB",
    "RUBYOPT",
    "RUBYLIB",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "CLASSPATH",
    "LUA_PATH",
    "LUA_CPATH",
    "LUA_INIT",
    "PHPRC",
    "PHP_INI_SCAN_DIR",
    "MAVEN_OPTS",
    "GRADLE_OPTS",
    "RUSTC",
    "RUSTFLAGS",
    "GOFLAGS",
];

const INTERPRETER_PREFIXES: &[&str] = &[
    "LD_",
    "DYLD_",
    "PYTHON",
    "GIT_",
    "DOTNET_",
    "COREHOST_",
    "COMPLUS_",
    "CORECLR_",
    "PERL5",
    "PERL_",
    "RUBY",
];

/// Variables that point a program at other places than the real ones
/// (profile and temp folders, proxies, certificate stores, the display) or that
/// belong to Sevak. A workflow may not set these; a plugin may still ask to
/// *inherit* Sevak's own value of the proxy and certificate ones.
const REDIRECT_VARS: &[&str] = &[
    "HOME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "USER",
    "USERNAME",
    "USERDOMAIN",
    "LOGNAME",
    "SHELL",
    "TEMP",
    "TMP",
    "TMPDIR",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "DBUS_SESSION_BUS_ADDRESS",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "FTP_PROXY",
    "NO_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
];

const REDIRECT_PREFIXES: &[&str] = &[
    "XDG_",
    "SEVAK_",
    "ALFRED_",
    "NODE_",
    "JAVA_",
    "GEM_",
    "BUNDLE_",
    "NPM_CONFIG_",
    "PIP_",
    "CARGO_",
    "RUSTC_",
];

fn listed(name: &str, exact: &[&str], prefixes: &[&str]) -> bool {
    // Names are compared without case on every platform: the rejecting lists
    // err on the strict side, and Windows has no case in names anyway.
    let upper = name.to_ascii_uppercase();
    exact.contains(&upper.as_str()) || prefixes.iter().any(|p| upper.starts_with(p))
}

/// Whether `name` makes an interpreter or the loader run something else (see
/// the module notes on `inherit_env`). Case-insensitive.
pub fn is_interpreter_variable(name: &str) -> bool {
    listed(name, INTERPRETER_VARS, INTERPRETER_PREFIXES)
}

/// Whether a workflow or script may not set `name` for a child process: the
/// [interpreter variables](is_interpreter_variable), plus the ones that move a
/// program's profile, temp folder, proxy, certificates or display, and
/// `SEVAK_*`. Case-insensitive.
pub fn is_reserved_variable(name: &str) -> bool {
    is_interpreter_variable(name) || listed(name, REDIRECT_VARS, REDIRECT_PREFIXES)
}

fn in_base_set(name: &str, case_insensitive: bool) -> bool {
    let eq = |a: &str, b: &str| {
        if case_insensitive {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    BASE_ENV.iter().any(|allowed| eq(name, allowed))
        || BASE_ENV_PREFIXES.iter().any(|prefix| {
            name.len() >= prefix.len()
                && name.is_char_boundary(prefix.len())
                && eq(&name[..prefix.len()], prefix)
        })
}

/// The part of `vars` a script process starts with: the base set, plus the
/// names in `inherit`. Interpreter variables are never passed on, even from
/// Sevak's own environment.
fn scrubbed_environment(
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    inherit: &[String],
    case_insensitive: bool,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    vars.into_iter()
        .filter(|(name, _)| {
            let Some(name) = name.to_str() else {
                return false;
            };
            // PATH, PATHEXT and COMSPEC are in the base set and are also on the
            // interpreter list (a workflow may not *set* them), so the base
            // set is checked first.
            in_base_set(name, case_insensitive)
                || (!is_interpreter_variable(name)
                    && inherit.iter().any(|extra| {
                        if case_insensitive {
                            extra.eq_ignore_ascii_case(name)
                        } else {
                            extra == name
                        }
                    }))
        })
        .collect()
}

/// Makes `command` start with a scrubbed environment: Sevak's own variables
/// that programs need to run (see `BASE_ENV`), the extra names in `inherit`
/// (a plugin's `inherit_env`), and nothing else. Call it **before** setting
/// any variable of your own on the command: it clears them.
pub fn scrub_environment(command: &mut Command, inherit: &[String]) {
    let kept = scrubbed_environment(env::vars_os(), inherit, cfg!(windows));
    command.env_clear();
    command.envs(kept);
}

/// Prepares a long-lived helper child that Sevak talks to over pipes (a script
/// plugin): the same child environment as [`spawn_detached`], and on Windows no
/// console window, which would otherwise flash for every console program.
pub fn configure_helper_command(command: &mut Command) {
    apply_child_env(command, crate::session::xwayland_forced());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

/// How to run a script file, by its extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptRunner {
    /// Run `<command...> <script path>`; the command is on `PATH`.
    Interpreter(Vec<String>),
    /// No interpreter is needed: start the file itself (an executable, a batch
    /// file, or a script with a shebang line on Unix).
    Direct,
    /// The extension needs an interpreter and none of these is installed.
    Missing(Vec<String>),
}

/// Picks how to run a script file with extension `extension` (no dot, any
/// case). Python runs unbuffered (`-u`), so output reaches Sevak without the
/// script having to flush.
pub fn script_runner(extension: &str) -> ScriptRunner {
    let candidates = interpreter_candidates(extension, cfg!(windows));
    if candidates.is_empty() {
        return ScriptRunner::Direct;
    }
    let names = candidates.iter().map(|c| c[0].to_owned()).collect();
    candidates
        .into_iter()
        .find(|candidate| find_in_path(candidate[0]).is_some())
        .map_or(ScriptRunner::Missing(names), |candidate| {
            ScriptRunner::Interpreter(candidate.into_iter().map(str::to_owned).collect())
        })
}

/// The interpreters to try for `extension`, best first. On Windows `python` and
/// `python3` may be Microsoft Store stubs that open the Store instead of
/// running anything, so the `py` launcher (which only exists for a real Python
/// install) goes first.
fn interpreter_candidates(extension: &str, windows: bool) -> Vec<Vec<&'static str>> {
    match (extension.to_ascii_lowercase().as_str(), windows) {
        ("py", true) => vec![
            vec!["py", "-3", "-u"],
            vec!["python", "-u"],
            vec!["python3", "-u"],
        ],
        ("py", false) => vec![vec!["python3", "-u"], vec!["python", "-u"]],
        ("ps1", true) => vec![
            vec!["pwsh", "-NoProfile", "-NonInteractive", "-File"],
            vec![
                "powershell",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ],
        ],
        ("ps1", false) => vec![vec!["pwsh", "-NoProfile", "-NonInteractive", "-File"]],
        ("js" | "mjs" | "cjs", _) => vec![vec!["node"]],
        ("sh", false) => vec![vec!["sh"]],
        _ => Vec::new(),
    }
}

fn detached_command<S: AsRef<OsStr>>(program: &str, args: &[S], cwd: Option<&Path>) -> Command {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    apply_child_env(&mut command, crate::session::xwayland_forced());
    command
}

fn spawn_error(program: &str, err: std::io::Error) -> PlatformError {
    PlatformError::CommandFailed {
        command: program.to_owned(),
        message: err.to_string(),
    }
}

/// Spawns `program` fully detached; see [`spawn_detached_in`].
pub fn spawn_detached<S: AsRef<OsStr>>(program: &str, args: &[S]) -> Result<()> {
    spawn_detached_in(program, args, None)
}

/// Spawns `program` fully detached: null stdio, its own process group (so a
/// Ctrl+C or SIGHUP aimed at Sevak does not reach it), and a reaper thread that
/// waits on the child so it never lingers as a zombie. `cwd` is the working
/// directory, if not Sevak's own.
#[cfg(unix)]
pub fn spawn_detached_in<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    cwd: Option<&Path>,
) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mut child = detached_command(program, args, cwd)
        .process_group(0)
        .spawn()
        .map_err(|err| spawn_error(program, err))?;

    let name = program.to_owned();
    let reaper = std::thread::Builder::new()
        .name("sevak-reaper".into())
        .spawn(move || match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => tracing::debug!(program = %name, %status, "detached child exited"),
            Err(err) => tracing::debug!(program = %name, %err, "waiting on detached child failed"),
        });
    if let Err(err) = reaper {
        // The child is already running; losing the reaper only risks a zombie.
        tracing::debug!(program, %err, "could not start reaper thread");
    }
    Ok(())
}

/// Spawns `program` fully detached: null stdio, no inherited console and its own
/// process group. Windows has no zombies, so the child is not waited on. `cwd`
/// is the working directory, if not Sevak's own.
#[cfg(windows)]
pub fn spawn_detached_in<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    cwd: Option<&Path>,
) -> Result<()> {
    use std::os::windows::process::CommandExt;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    detached_command(program, args, cwd)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(drop)
        .map_err(|err| spawn_error(program, err))
}

/// Runs `program` and reports an immediate failure.
///
/// Waits up to `grace` for the program to exit. A non-zero exit inside that
/// time becomes a [`PlatformError::CommandFailed`] carrying the program's
/// stderr; a program still running afterwards (a slow job, or a window that
/// stays open) is left to finish in the background and counts as started.
pub fn run_checked<S: AsRef<OsStr>>(program: &str, args: &[S], grace: Duration) -> Result<()> {
    use std::io::Read;
    use std::sync::mpsc;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    apply_child_env(&mut command, crate::session::xwayland_forced());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Console tools such as shutdown.exe would flash a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|err| spawn_error(program, err))?;

    // A thread owns the child so a slow program never blocks the caller and is
    // still reaped; it reports the exit through the channel if anyone listens.
    let (tx, rx) = mpsc::channel();
    let name = program.to_owned();
    let waiter = std::thread::Builder::new()
        .name("sevak-run".into())
        .spawn(move || {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            let status = child.wait();
            if let Err(err) = &status {
                tracing::debug!(program = %name, %err, "waiting on child failed");
            }
            let _ = tx.send((status, stderr));
        });
    if let Err(err) = waiter {
        tracing::debug!(program, %err, "could not start waiter thread");
        return Ok(());
    }

    match rx.recv_timeout(grace) {
        Ok((Ok(status), _)) if status.success() => Ok(()),
        Ok((Ok(status), stderr)) => {
            let stderr = stderr.trim();
            Err(PlatformError::CommandFailed {
                command: program.to_owned(),
                message: if stderr.is_empty() {
                    status.to_string()
                } else {
                    format!("{status}: {stderr}")
                },
            })
        }
        Ok((Err(err), _)) => Err(spawn_error(program, err)),
        // Still running, or the waiter vanished: nothing failed yet.
        Err(_) => Ok(()),
    }
}

/// Spawns a console program (a shell) in a new console window of its own.
///
/// Unlike [`spawn_detached_in`] the child's standard handles are *not* nulled:
/// the new console supplies them, and a shell with stdin redirected to `NUL`
/// would exit at once. Sevak is a GUI process without a console, so there are no
/// inherited handles to leak. `raw_tail` is appended to the command line as is
/// (see [`crate::terminal::Invocation::raw_tail`]).
#[cfg(windows)]
pub fn spawn_console_in<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    raw_tail: Option<&str>,
    cwd: Option<&Path>,
) -> Result<()> {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let mut command = Command::new(program);
    command.args(args);
    if let Some(raw_tail) = raw_tail {
        command.raw_arg(raw_tail);
    }
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command
        .creation_flags(CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(drop)
        .map_err(|err| spawn_error(program, err))
}

/// Lets the already-running Sevak instance bring its window to the foreground
/// when this (second) process forwards it `--toggle`.
///
/// Windows only lets the foreground process activate windows; a freshly launched
/// second instance has that right and passes it on. No-op elsewhere.
pub fn allow_foreground_handoff() {
    #[cfg(windows)]
    crate::windows::allow_foreground_handoff();
}

/// Attaches to the parent console so release builds (GUI subsystem, no console)
/// can print `--help` output. No-op elsewhere.
pub fn attach_parent_console() {
    #[cfg(windows)]
    crate::windows::attach_parent_console();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nothing_for_nonsense_program() {
        assert!(find_in_path("sevak-definitely-not-a-real-program").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn finds_cmd_without_extension() {
        assert!(find_in_path("cmd").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn finds_sh() {
        assert!(find_in_path("sh").is_some());
    }

    fn os(pairs: &[(&str, &str)]) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }

    fn names(kept: &[(std::ffi::OsString, std::ffi::OsString)]) -> Vec<String> {
        let mut names: Vec<String> = kept
            .iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_script_starts_with_the_base_environment_and_nothing_else() {
        let vars = os(&[
            ("PATH", "/bin"),
            ("HOME", "/home/me"),
            ("LANG", "en_US.UTF-8"),
            ("LC_ALL", "C"),
            ("XDG_CONFIG_HOME", "/c"),
            ("DISPLAY", ":0"),
            ("TEMP", "/tmp"),
            ("AWS_SECRET_ACCESS_KEY", "s"),
            ("GITHUB_TOKEN", "t"),
            ("HTTPS_PROXY", "http://p"),
            ("BASH_ENV", "/x"),
            ("PYTHONSTARTUP", "/x"),
            ("LD_PRELOAD", "/x"),
        ]);
        let kept = scrubbed_environment(vars, &[], false);
        assert_eq!(
            names(&kept),
            [
                "DISPLAY",
                "HOME",
                "LANG",
                "LC_ALL",
                "PATH",
                "TEMP",
                "XDG_CONFIG_HOME"
            ]
        );
    }

    #[test]
    fn inherit_env_adds_names_but_never_interpreter_variables() {
        let vars = os(&[
            ("PATH", "/bin"),
            ("OPENAI_API_KEY", "k"),
            ("HTTPS_PROXY", "http://p"),
            ("BASH_ENV", "/x"),
            ("NODE_OPTIONS", "--require x"),
            ("OTHER", "o"),
        ]);
        let inherit: Vec<String> = ["OPENAI_API_KEY", "HTTPS_PROXY", "BASH_ENV", "NODE_OPTIONS"]
            .map(String::from)
            .into();
        let kept = scrubbed_environment(vars, &inherit, false);
        assert_eq!(names(&kept), ["HTTPS_PROXY", "OPENAI_API_KEY", "PATH"]);
    }

    #[test]
    fn names_have_no_case_on_windows_only() {
        let vars = || {
            os(&[
                ("Path", "C:\\bin"),
                ("SystemRoot", "C:\\Windows"),
                ("Secret", "s"),
            ])
        };
        assert_eq!(
            names(&scrubbed_environment(vars(), &[], true)),
            ["Path", "SystemRoot"]
        );
        assert!(
            scrubbed_environment(vars(), &[], false).is_empty(),
            "on Unix `Path` is not `PATH`"
        );
        let inherit = ["SECRET".to_owned()];
        assert_eq!(
            names(&scrubbed_environment(vars(), &inherit, true)),
            ["Path", "Secret", "SystemRoot"]
        );
    }

    #[test]
    fn scrubbing_a_command_removes_what_it_was_given_before() {
        let mut command = Command::new("x");
        command.env("LEFTOVER", "1");
        scrub_environment(&mut command, &[]);
        assert!(!command.get_envs().any(|(k, _)| k == "LEFTOVER"));
        assert!(command
            .get_envs()
            .any(|(k, _)| k.eq_ignore_ascii_case("PATH")));
    }

    #[test]
    fn variables_that_change_how_programs_start_are_refused() {
        for name in [
            "BASH_ENV",
            "bash_env",
            "ENV",
            "PYTHONSTARTUP",
            "PythonPath",
            "PYTHONHOME",
            "NODE_OPTIONS",
            "NODE_PATH",
            "PERL5OPT",
            "PERL5LIB",
            "RUBYOPT",
            "RUBYLIB",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "PATH",
            "Path",
            "PATHEXT",
            "COMSPEC",
            "ComSpec",
            "PSModulePath",
            "GIT_SSH_COMMAND",
            "DOTNET_STARTUP_HOOKS",
        ] {
            assert!(is_interpreter_variable(name), "{name}");
            assert!(is_reserved_variable(name), "{name}");
        }
        for name in [
            "HTTPS_PROXY",
            "SSL_CERT_FILE",
            "TMPDIR",
            "HOME",
            "userprofile",
            "SEVAK_QUERY",
            "XDG_DATA_HOME",
            "NODE_ENV",
        ] {
            assert!(is_reserved_variable(name), "{name}");
        }
        for name in ["greeting", "API_KEY", "LANG", "my_var", "OPENAI_API_KEY"] {
            assert!(!is_reserved_variable(name), "{name}");
        }
        // Reserved but not an interpreter hook: a manifest may inherit these.
        assert!(!is_interpreter_variable("HTTPS_PROXY"));
        assert!(!is_interpreter_variable("NODE_ENV"));
    }

    #[test]
    fn gdk_backend_is_removed_only_when_sevak_forced_it() {
        let removed = |forced: bool| {
            let mut command = Command::new("x");
            apply_child_env(&mut command, forced);
            command
                .get_envs()
                .any(|(key, value)| key == "GDK_BACKEND" && value.is_none())
        };
        assert!(removed(true));
        assert!(!removed(false));
    }

    fn shell(script: &str) -> (&'static str, Vec<String>) {
        if cfg!(windows) {
            ("cmd", vec!["/c".into(), script.into()])
        } else {
            ("sh", vec!["-c".into(), script.into()])
        }
    }

    #[test]
    fn run_checked_succeeds_for_a_clean_exit() {
        let (program, args) = shell("exit 0");
        assert!(run_checked(program, &args, Duration::from_secs(10)).is_ok());
    }

    #[test]
    fn run_checked_reports_a_failure_with_its_stderr() {
        let (program, args) = shell(if cfg!(windows) {
            "echo boom 1>&2 & exit 3"
        } else {
            "echo boom >&2; exit 3"
        });
        let err = run_checked(program, &args, Duration::from_secs(10)).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("boom"), "{text}");
        assert!(matches!(err, PlatformError::CommandFailed { .. }));
    }

    #[test]
    fn run_checked_does_not_wait_for_a_slow_program() {
        let (program, args) = if cfg!(windows) {
            (
                "cmd",
                vec!["/c".to_owned(), "ping -n 6 127.0.0.1 >nul".to_owned()],
            )
        } else {
            ("sh", vec!["-c".to_owned(), "sleep 5".to_owned()])
        };
        let started = std::time::Instant::now();
        assert!(run_checked(program, &args, Duration::from_millis(100)).is_ok());
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn run_checked_of_missing_program_is_an_error() {
        let result = run_checked(
            "sevak-definitely-not-a-real-program",
            &[] as &[&str],
            Duration::from_secs(1),
        );
        assert!(matches!(result, Err(PlatformError::CommandFailed { .. })));
    }

    #[test]
    fn interpreter_by_extension() {
        let first = |ext, windows| interpreter_candidates(ext, windows).remove(0);
        assert_eq!(first("py", true), ["py", "-3", "-u"]);
        assert_eq!(first("PY", false), ["python3", "-u"]);
        assert_eq!(first("js", true), ["node"]);
        assert_eq!(first("ps1", true)[0], "pwsh");
        assert!(interpreter_candidates("sh", true).is_empty());
        assert!(interpreter_candidates("exe", true).is_empty());
        assert!(interpreter_candidates("", false).is_empty());
        assert_eq!(script_runner("exe"), ScriptRunner::Direct);
        // Whatever is found must be a command that exists.
        match script_runner("py") {
            ScriptRunner::Interpreter(argv) => assert!(find_in_path(&argv[0]).is_some()),
            ScriptRunner::Missing(names) => assert!(!names.is_empty()),
            ScriptRunner::Direct => panic!("python scripts need an interpreter"),
        }
    }

    #[test]
    fn spawn_of_missing_program_is_an_error() {
        let result = spawn_detached("sevak-definitely-not-a-real-program", &[] as &[&str]);
        assert!(matches!(result, Err(PlatformError::CommandFailed { .. })));
    }
}
