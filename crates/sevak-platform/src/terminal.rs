//! Opening a terminal window that runs a shell command (the `>` plugin).
//!
//! Building the command line is split from launching it. The `plan_*`
//! functions are pure: they take the command, the `[shell]` config and a
//! [`Probe`] (which programs exist, which environment variables are set) and
//! return the [`Invocation`] to spawn. They compile and are unit-tested on every
//! OS, so the Linux and macOS argument construction is checked from Windows
//! too. [`run_in_terminal`] picks the plan for the current OS, probes the real
//! machine and spawns the result.
//!
//! An empty command means "just open a terminal".
//!
//! | OS | Terminal | Command line |
//! |---|---|---|
//! | Windows | `wt.exe`, else a plain console window | `wt new-tab -- <shell> <args>`; PowerShell gets the command as `-EncodedCommand`, so no quoting is needed |
//! | macOS | Terminal.app or iTerm2 | `osascript` running `do script` / `write text` |
//! | Linux | `$TERMINAL`, then a list of common terminals | `<terminal> <execute flag> <shell> -c <command>` |

use std::path::Path;

use sevak_core::config::ShellConfig;

use crate::error::{PlatformError, Result};
use crate::process::find_in_path;

/// A program to spawn, ready for [`crate::process`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    /// Windows console shells only: text appended to the command line as is.
    /// `cmd.exe` does not parse quotes like other programs, so its command is
    /// passed verbatim (`/S /K "command"`) instead of through argv quoting.
    pub raw_tail: Option<String>,
    /// Windows only: run in a console window of its own. Without a terminal
    /// emulator the shell is a console program, and Sevak has no console.
    pub new_console: bool,
}

impl Invocation {
    fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            raw_tail: None,
            new_console: false,
        }
    }
}

/// What a plan needs to know about the machine.
pub struct Probe<'a> {
    /// Resolves a program name or path to something that can be spawned, or
    /// `None` when it is not installed.
    pub find: &'a dyn Fn(&str) -> Option<String>,
    /// Reads an environment variable (`None` when unset or empty).
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

/// Runs `f` with a [`Probe`] of the real machine.
fn with_system_probe<T>(f: impl FnOnce(&Probe) -> T) -> T {
    let find = |program: &str| -> Option<String> {
        let path = Path::new(program);
        if path.components().count() > 1 {
            // Already a path (relative or absolute).
            return path.is_file().then(|| program.to_owned());
        }
        find_in_path(program).map(|found| found.to_string_lossy().into_owned())
    };
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    f(&Probe {
        find: &find,
        env: &env,
    })
}

/// Opens a terminal at a shell prompt in `dir` (an absolute path). The terminal
/// stays open whatever `[shell] keep_open` says: there is no command whose end
/// would close it, and a window that closes at once is no use.
pub fn open_terminal_in(dir: &Path, config: &ShellConfig) -> Result<()> {
    let mut config = config.clone();
    config.keep_open = true;
    let kind = with_system_probe(|probe| current_shell_kind(&config, probe))?;
    let command = cd_command(kind, dir)?;
    run_in_terminal(&command, &config)
}

/// Opens a terminal and runs `command` in it according to `config`.
pub fn run_in_terminal(command: &str, config: &ShellConfig) -> Result<()> {
    let command = command.trim();
    if command.contains('\0') {
        return Err(terminal_error("the command contains a NUL character"));
    }
    with_system_probe(|probe| run_planned(command, config, probe))
}

fn run_planned(command: &str, config: &ShellConfig, probe: &Probe) -> Result<()> {
    #[cfg(windows)]
    let invocation = plan_windows(command, config, probe)?;
    #[cfg(target_os = "macos")]
    let invocation = plan_macos(command, config)?;
    #[cfg(target_os = "linux")]
    let invocation = plan_linux(command, config, probe)?;
    #[cfg(target_os = "macos")]
    let _ = probe;

    tracing::debug!(program = %invocation.program, "opening terminal");
    // Terminals start where the user's own terminals do.
    spawn(&invocation, dirs::home_dir().as_deref())
}

fn spawn(invocation: &Invocation, cwd: Option<&Path>) -> Result<()> {
    #[cfg(windows)]
    if invocation.new_console {
        return crate::process::spawn_console_in(
            &invocation.program,
            &invocation.args,
            invocation.raw_tail.as_deref(),
            cwd,
        );
    }
    crate::process::spawn_detached_in(&invocation.program, &invocation.args, cwd)
}

fn terminal_error(message: impl Into<String>) -> PlatformError {
    PlatformError::Os {
        operation: "run_in_terminal",
        message: message.into(),
    }
}

// ---------------------------------------------------------------------------
// Shells
// ---------------------------------------------------------------------------

/// How a shell takes a command, decided by its program name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellKind {
    /// `pwsh` / `powershell`: `-NoExit -EncodedCommand <base64>`.
    PowerShell,
    /// `cmd`: `/S /K <command>`.
    Cmd,
    /// `sh`, `bash`, `zsh`, `fish`, ...: `-c <script>`.
    Posix,
}

/// The lowercase file name of `program` without directory and `.exe`.
fn program_stem(program: &str) -> String {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_owned()
}

fn shell_kind(shell: &str) -> ShellKind {
    match program_stem(shell).as_str() {
        "pwsh" | "powershell" => ShellKind::PowerShell,
        "cmd" => ShellKind::Cmd,
        _ => ShellKind::Posix,
    }
}

/// How a value must be quoted to reach the shell [`run_in_terminal`] starts as
/// one literal word, with nothing in it interpreted (workflows' Terminal
/// command node quotes the text it inserts this way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellQuoting {
    /// `sh`, `bash`, `zsh`, `fish`, ...: single quotes.
    Posix,
    /// `pwsh` / `powershell`: single quotes, with every kind of single quote
    /// doubled.
    PowerShell,
    /// `cmd.exe`: double quotes, and some characters cannot be quoted at all.
    Cmd,
}

impl From<ShellKind> for ShellQuoting {
    fn from(kind: ShellKind) -> Self {
        match kind {
            ShellKind::PowerShell => Self::PowerShell,
            ShellKind::Cmd => Self::Cmd,
            ShellKind::Posix => Self::Posix,
        }
    }
}

/// The quoting the shell that [`run_in_terminal`] would start with `config`
/// needs, found the same way (`[shell] shell`, then `PATH` / `$SHELL`). A
/// configured shell that is missing is judged by its name (running it fails
/// anyway). Probes the system; cheap.
pub fn shell_quoting(config: &ShellConfig) -> ShellQuoting {
    with_system_probe(|probe| current_shell_kind(config, probe))
        .unwrap_or_else(|_| shell_kind(&config.shell))
        .into()
}

/// The kind of the shell [`run_in_terminal`] would use on this OS.
fn current_shell_kind(config: &ShellConfig, probe: &Probe) -> Result<ShellKind> {
    #[cfg(windows)]
    let shell = windows_shell(config, probe)?;
    #[cfg(target_os = "linux")]
    let shell = linux_shell(config, probe)?;
    // macOS runs the user's login shell; every one of them takes `cd 'dir'`.
    #[cfg(target_os = "macos")]
    let shell = {
        let _ = (config, probe);
        "sh".to_owned()
    };
    Ok(shell_kind(&shell))
}

/// The command that makes a shell of `kind` change to `dir`.
fn cd_command(kind: ShellKind, dir: &Path) -> Result<String> {
    if !dir.is_absolute() {
        return Err(terminal_error("the folder must be an absolute path"));
    }
    let dir = dir.to_string_lossy();
    if dir.chars().any(|c| c == '\0' || c == '\n' || c == '\r') {
        return Err(terminal_error("the folder name contains a line break"));
    }
    Ok(match kind {
        ShellKind::PowerShell => {
            // Every kind of single quote ends a PowerShell string; doubling
            // one keeps it literal.
            let mut quoted = String::with_capacity(dir.len() + 2);
            for c in dir.chars() {
                quoted.push(c);
                if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
                    quoted.push(c);
                }
            }
            format!("Set-Location -LiteralPath '{quoted}'")
        }
        ShellKind::Cmd => {
            // Neither can be escaped on cmd's command line.
            if dir.contains('"') || dir.contains('%') {
                return Err(terminal_error(
                    "cmd cannot change to a folder whose name contains \" or %",
                ));
            }
            format!("cd /d \"{dir}\"")
        }
        ShellKind::Posix => format!("cd {}", sh_quote(&dir)),
    })
}

/// Single-quotes `text` for a POSIX shell (also valid in fish).
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Standard base64 (with padding) of `bytes`.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// What PowerShell's `-EncodedCommand` expects: base64 of the UTF-16LE text.
fn powershell_encoded(command: &str) -> String {
    let bytes: Vec<u8> = command.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

/// Arguments that make `shell` (of `kind`) run `command`, and stay at a prompt
/// afterwards when `keep_open`. An empty command gives an interactive shell.
fn shell_args(kind: ShellKind, shell: &str, command: &str, keep_open: bool) -> Vec<String> {
    let owned = |args: &[&str]| args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
    match kind {
        ShellKind::PowerShell => {
            let mut args = owned(&["-NoLogo"]);
            if !command.is_empty() {
                if keep_open {
                    args.push("-NoExit".to_owned());
                }
                args.push("-EncodedCommand".to_owned());
                args.push(powershell_encoded(command));
            }
            args
        }
        ShellKind::Cmd => {
            if command.is_empty() {
                return Vec::new();
            }
            let mut args = owned(&["/S", if keep_open { "/K" } else { "/C" }]);
            args.push(command.to_owned());
            args
        }
        ShellKind::Posix => {
            if command.is_empty() {
                return Vec::new();
            }
            // A newline (not `;`) so a trailing `# comment` or `&` in the
            // command cannot swallow the `exec`.
            let script = if keep_open {
                format!("{command}\nexec {}", sh_quote(shell))
            } else {
                command.to_owned()
            };
            vec!["-c".to_owned(), script]
        }
    }
}

/// The `cmd.exe` argument tail that keeps the command's own quotes intact:
/// `/S` strips exactly the outer pair.
fn cmd_raw_tail(command: &str, keep_open: bool) -> String {
    format!("/S {} \"{command}\"", if keep_open { "/K" } else { "/C" })
}

// ---------------------------------------------------------------------------
// Terminal emulators
// ---------------------------------------------------------------------------

/// How a terminal emulator is told which program to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// `wt.exe new-tab -- <program> <args>`; semicolons are wt's command
    /// separator and must be escaped.
    WindowsTerminal,
    /// `<terminal> -e <program> <args>` (xterm, konsole, alacritty, ...).
    DashE,
    /// `<terminal> -x <program> <args>` (xfce4-terminal, mate-terminal, ...).
    DashX,
    /// `<terminal> -- <program> <args>` (gnome-terminal).
    DoubleDash,
    /// `<terminal> <program> <args>` (kitty, foot).
    Direct,
    /// `wezterm start -- <program> <args>`.
    WezTerm,
    /// The Windows console host: no emulator, the shell gets its own window.
    Console,
}

fn style_of(terminal: &str) -> Style {
    match program_stem(terminal).as_str() {
        "wt" => Style::WindowsTerminal,
        "gnome-terminal" => Style::DoubleDash,
        "xfce4-terminal" | "mate-terminal" | "terminator" => Style::DashX,
        "kitty" | "foot" | "footclient" => Style::Direct,
        "wezterm" | "wezterm-gui" => Style::WezTerm,
        "conhost" => Style::Console,
        // x-terminal-emulator, xterm, konsole, alacritty, tilix, st, urxvt...
        _ => Style::DashE,
    }
}

/// The arguments between the terminal and the program it should run.
fn execute_flag(style: Style) -> &'static [&'static str] {
    match style {
        Style::WindowsTerminal => &["new-tab", "--"],
        Style::DashE => &["-e"],
        Style::DashX => &["-x"],
        Style::DoubleDash => &["--"],
        Style::Direct | Style::Console => &[],
        Style::WezTerm => &["start", "--"],
    }
}

/// Splits a configured terminal like `alacritty --class sevak` into program and
/// arguments. Single and double quotes group words; there are no escapes, so
/// Windows paths keep their backslashes.
fn split_command_line(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for ch in text.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(ch);
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            (None, c) => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

/// A configured terminal: the resolved program, its own extra arguments and how
/// to hand it a command.
struct Terminal {
    program: String,
    extra_args: Vec<String>,
    style: Style,
}

/// Parses `[shell] terminal`. The whole text is first tried as a program path,
/// so `C:\Program Files\...\foot.exe` works unquoted.
fn configured_terminal(text: &str, probe: &Probe) -> Result<Terminal> {
    let (program, extra_args) = match (probe.find)(text) {
        Some(found) => (found, Vec::new()),
        None => {
            let mut words = split_command_line(text);
            if words.is_empty() {
                return Err(terminal_error("[shell] terminal is empty"));
            }
            let program = words.remove(0);
            let Some(found) = (probe.find)(&program) else {
                return Err(terminal_error(format!(
                    "the terminal `{program}` set in [shell] terminal was not found"
                )));
            };
            (found, words)
        }
    };
    let style = style_of(&program);
    Ok(Terminal {
        program,
        extra_args,
        style,
    })
}

fn terminal_invocation(terminal: &Terminal, shell: Option<(String, Vec<String>)>) -> Invocation {
    let mut args = terminal.extra_args.clone();
    // Without a shell there is nothing to run: just the terminal.
    if let Some((shell_program, shell_args)) = shell {
        args.extend(execute_flag(terminal.style).iter().map(|a| (*a).to_owned()));
        args.push(shell_program);
        args.extend(shell_args);
        if terminal.style == Style::WindowsTerminal {
            // `;` separates wt's own commands; `\;` is a literal semicolon.
            for arg in &mut args {
                *arg = arg.replace(';', r"\;");
            }
        }
    }
    Invocation::new(terminal.program.clone(), args)
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// Windows: Windows Terminal when installed, else a console window; PowerShell
/// 7, then Windows PowerShell, then `cmd` as the shell.
pub fn plan_windows(command: &str, config: &ShellConfig, probe: &Probe) -> Result<Invocation> {
    let shell = windows_shell(config, probe)?;
    let kind = shell_kind(&shell);
    let args = shell_args(kind, &shell, command, config.keep_open);

    let terminal = if config.terminal.is_empty() {
        (probe.find)("wt").map(|program| Terminal {
            program,
            extra_args: Vec::new(),
            style: Style::WindowsTerminal,
        })
    } else {
        Some(configured_terminal(&config.terminal, probe)?)
    };

    match terminal {
        Some(terminal) if terminal.style != Style::Console => {
            let shell = (!command.is_empty()).then_some((shell, args));
            Ok(terminal_invocation(&terminal, shell))
        }
        _ => {
            let mut invocation = Invocation::new(shell, args);
            invocation.new_console = true;
            if kind == ShellKind::Cmd && !command.is_empty() {
                invocation.args.clear();
                invocation.raw_tail = Some(cmd_raw_tail(command, config.keep_open));
            }
            Ok(invocation)
        }
    }
}

/// The shell on Windows: `[shell] shell`, else PowerShell 7, Windows
/// PowerShell, then `cmd`.
fn windows_shell(config: &ShellConfig, probe: &Probe) -> Result<String> {
    if config.shell.is_empty() {
        Ok(["pwsh", "powershell"]
            .into_iter()
            .find_map(|name| (probe.find)(name))
            .or_else(|| (probe.env)("COMSPEC"))
            .unwrap_or_else(|| "cmd.exe".to_owned()))
    } else {
        (probe.find)(&config.shell).ok_or_else(|| {
            terminal_error(format!(
                "the shell `{}` set in [shell] shell was not found",
                config.shell
            ))
        })
    }
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

/// Escapes `text` for an AppleScript string literal.
fn applescript_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str(r"\\"),
            '"' => out.push_str(r#"\""#),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// macOS: Terminal.app (default) or iTerm2, driven through `osascript`. The
/// terminal starts the user's login shell, which runs the command, so
/// `[shell] shell` does not apply.
pub fn plan_macos(command: &str, config: &ShellConfig) -> Result<Invocation> {
    let name = program_stem(config.terminal.trim_end_matches('/'));
    let name = name.strip_suffix(".app").unwrap_or(&name);
    // `; exit` closes the window (per the terminal's own "close when the shell
    // exits" setting) instead of leaving a prompt.
    let text = if config.keep_open || command.is_empty() {
        command.to_owned()
    } else {
        format!("{command}; exit")
    };
    let text = applescript_escape(&text);

    let lines: Vec<String> = match name {
        "" | "terminal" => {
            let script = if command.is_empty() {
                "do script".to_owned()
            } else {
                format!("do script \"{text}\"")
            };
            vec![
                "tell application \"Terminal\"".to_owned(),
                script,
                "activate".to_owned(),
                "end tell".to_owned(),
            ]
        }
        "iterm" | "iterm2" => {
            let mut lines = vec![
                "tell application \"iTerm\"".to_owned(),
                "activate".to_owned(),
                "set sevakWindow to (create window with default profile)".to_owned(),
            ];
            if !command.is_empty() {
                lines.push(format!(
                    "tell current session of sevakWindow to write text \"{text}\""
                ));
            }
            lines.push("end tell".to_owned());
            lines
        }
        other => {
            return Err(terminal_error(format!(
                "the terminal `{other}` is not supported on macOS; use \"terminal\" or \"iterm\""
            )))
        }
    };
    let args = lines
        .into_iter()
        .flat_map(|line| ["-e".to_owned(), line])
        .collect();
    Ok(Invocation::new("osascript", args))
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

/// Terminals tried, in order, when neither `[shell] terminal` nor `$TERMINAL`
/// names one.
const LINUX_TERMINALS: &[&str] = &[
    "x-terminal-emulator",
    "gnome-terminal",
    "konsole",
    "kitty",
    "alacritty",
    "wezterm",
    "foot",
    "xterm",
];

/// Linux: `[shell] terminal`, else `$TERMINAL`, else the first installed of
/// [`LINUX_TERMINALS`]; the shell is `[shell] shell`, else `$SHELL`, else `sh`.
pub fn plan_linux(command: &str, config: &ShellConfig, probe: &Probe) -> Result<Invocation> {
    let terminal = if !config.terminal.is_empty() {
        configured_terminal(&config.terminal, probe)?
    } else if let Some(terminal) =
        (probe.env)("TERMINAL").and_then(|text| configured_terminal(&text, probe).ok())
    {
        terminal
    } else {
        let program = LINUX_TERMINALS
            .iter()
            .find_map(|name| (probe.find)(name))
            .ok_or_else(|| {
                terminal_error(
                    "no terminal found; install one or set [shell] terminal in the config",
                )
            })?;
        let style = style_of(&program);
        Terminal {
            program,
            extra_args: Vec::new(),
            style,
        }
    };

    let shell = linux_shell(config, probe)?;
    let args = shell_args(shell_kind(&shell), &shell, command, config.keep_open);
    let shell = (!command.is_empty()).then_some((shell, args));
    Ok(terminal_invocation(&terminal, shell))
}

/// The shell on Linux: `[shell] shell`, else `$SHELL`, else `sh`.
fn linux_shell(config: &ShellConfig, probe: &Probe) -> Result<String> {
    if config.shell.is_empty() {
        Ok((probe.env)("SHELL")
            .and_then(|shell| (probe.find)(&shell))
            .unwrap_or_else(|| "sh".to_owned()))
    } else {
        (probe.find)(&config.shell).ok_or_else(|| {
            terminal_error(format!(
                "the shell `{}` set in [shell] shell was not found",
                config.shell
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine with the given programs installed and variables set.
    struct Machine {
        programs: &'static [&'static str],
        env: &'static [(&'static str, &'static str)],
    }

    impl Machine {
        fn plan(&self, f: impl FnOnce(&Probe) -> Result<Invocation>) -> Result<Invocation> {
            let find = |program: &str| {
                self.programs
                    .iter()
                    .find(|p| **p == program || **p == format!("{program}.exe"))
                    .map(|p| (*p).to_owned())
            };
            let env = |name: &str| {
                self.env
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            };
            f(&Probe {
                find: &find,
                env: &env,
            })
        }
    }

    fn config(terminal: &str, shell: &str, keep_open: bool) -> ShellConfig {
        ShellConfig {
            terminal: terminal.to_owned(),
            shell: shell.to_owned(),
            keep_open,
        }
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn powershell_commands_are_utf16le_base64() {
        assert_eq!(powershell_encoded("a"), "YQA=");
        assert_eq!(powershell_encoded("hi"), "aABpAA==");
        // Non-ASCII characters survive as UTF-16 code units.
        assert_eq!(powershell_encoded("\u{e9}"), "6QA=");
    }

    #[test]
    fn shell_kinds_by_program_name() {
        assert_eq!(
            shell_kind(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            ShellKind::PowerShell
        );
        assert_eq!(shell_kind("PowerShell.EXE"), ShellKind::PowerShell);
        assert_eq!(shell_kind("cmd.exe"), ShellKind::Cmd);
        assert_eq!(shell_kind("/usr/bin/fish"), ShellKind::Posix);
        assert_eq!(shell_kind("C:\\Git\\bin\\bash.exe"), ShellKind::Posix);
    }

    /// An absolute path on whichever OS the tests run, with `name` as its
    /// last component.
    fn absolute(name: &str) -> std::path::PathBuf {
        let root = if cfg!(windows) {
            r"C:\Users\me"
        } else {
            "/home/me"
        };
        Path::new(root).join(name)
    }

    #[test]
    fn changing_directory_is_quoted_for_each_shell() {
        let dir = absolute("My Dir");
        let shown = dir.to_string_lossy().into_owned();
        assert_eq!(
            cd_command(ShellKind::Posix, &dir).unwrap(),
            format!("cd '{shown}'")
        );
        assert_eq!(
            cd_command(ShellKind::PowerShell, &dir).unwrap(),
            format!("Set-Location -LiteralPath '{shown}'")
        );
        assert_eq!(
            cd_command(ShellKind::Cmd, &dir).unwrap(),
            format!("cd /d \"{shown}\"")
        );
    }

    #[test]
    fn awkward_folder_names_stay_literal() {
        let quote = absolute("it's");
        let shown = quote.to_string_lossy().into_owned();
        assert_eq!(
            cd_command(ShellKind::Posix, &quote).unwrap(),
            format!("cd {}", sh_quote(&shown))
        );
        // PowerShell doubles every kind of single quote.
        assert!(cd_command(ShellKind::PowerShell, &quote)
            .unwrap()
            .ends_with("it''s'"));
        assert!(cd_command(ShellKind::PowerShell, &absolute("it\u{2019}s"))
            .unwrap()
            .ends_with("it\u{2019}\u{2019}s'"));
        // cmd cannot take these at all.
        assert!(cd_command(ShellKind::Cmd, &absolute("100%")).is_err());
        assert!(cd_command(ShellKind::Cmd, &absolute("a\"b")).is_err());
        // Nor can any shell take a line break.
        assert!(cd_command(ShellKind::Posix, &absolute("a\nb")).is_err());
    }

    #[test]
    fn changing_directory_needs_an_absolute_path() {
        assert!(cd_command(ShellKind::Posix, Path::new("relative/dir")).is_err());
        assert!(cd_command(ShellKind::Posix, Path::new("")).is_err());
    }

    #[test]
    fn the_shell_is_chosen_like_the_plans_choose_it() {
        let find = |program: &str| {
            ["pwsh.exe", "/bin/zsh"]
                .contains(&program)
                .then(|| program.to_owned())
        };
        let env = |name: &str| (name == "SHELL").then(|| "/bin/zsh".to_owned());
        let probe = Probe {
            find: &find,
            env: &env,
        };
        assert_eq!(
            windows_shell(&config("", "pwsh.exe", true), &probe).unwrap(),
            "pwsh.exe"
        );
        assert!(windows_shell(&config("", "nope", true), &probe).is_err());
        assert_eq!(
            linux_shell(&config("", "", true), &probe).unwrap(),
            "/bin/zsh"
        );
        assert!(linux_shell(&config("", "nope", true), &probe).is_err());
    }

    #[test]
    fn posix_scripts_quote_the_shell_and_keep_the_command_verbatim() {
        let args = shell_args(ShellKind::Posix, "/bin/zsh", "echo \"a b\" # hi", true);
        assert_eq!(args, vec!["-c", "echo \"a b\" # hi\nexec '/bin/zsh'"]);
        let args = shell_args(ShellKind::Posix, "/odd/it's/sh", "ls", true);
        assert_eq!(args[1], "ls\nexec '/odd/it'\\''s/sh'");
        let args = shell_args(ShellKind::Posix, "/bin/sh", "ls", false);
        assert_eq!(args, vec!["-c", "ls"]);
        assert!(shell_args(ShellKind::Posix, "/bin/sh", "", true).is_empty());
    }

    #[test]
    fn powershell_args_follow_keep_open() {
        let enc = powershell_encoded("dir");
        assert_eq!(
            shell_args(ShellKind::PowerShell, "pwsh", "dir", true),
            vec!["-NoLogo", "-NoExit", "-EncodedCommand", &enc]
        );
        assert_eq!(
            shell_args(ShellKind::PowerShell, "pwsh", "dir", false),
            vec!["-NoLogo", "-EncodedCommand", &enc]
        );
        assert_eq!(
            shell_args(ShellKind::PowerShell, "pwsh", "", true),
            vec!["-NoLogo"]
        );
    }

    #[test]
    fn cmd_args_and_raw_tail() {
        assert_eq!(
            shell_args(ShellKind::Cmd, "cmd.exe", "dir", true),
            vec!["/S", "/K", "dir"]
        );
        assert_eq!(
            shell_args(ShellKind::Cmd, "cmd.exe", "dir", false),
            vec!["/S", "/C", "dir"]
        );
        assert_eq!(
            cmd_raw_tail(r#"echo "a b" & dir"#, true),
            r#"/S /K "echo "a b" & dir""#
        );
        assert_eq!(cmd_raw_tail("dir", false), r#"/S /C "dir""#);
    }

    #[test]
    fn command_lines_split_with_quotes() {
        assert_eq!(
            split_command_line("alacritty --class sevak"),
            strings(&["alacritty", "--class", "sevak"])
        );
        assert_eq!(
            split_command_line(r#"  "C:\Program Files\Foo\foo.exe"  -x 'a b' "#),
            strings(&[r"C:\Program Files\Foo\foo.exe", "-x", "a b"])
        );
        assert_eq!(split_command_line(r#""" x"#), strings(&["", "x"]));
        assert!(split_command_line("   ").is_empty());
    }

    // ----- Windows -----

    const WINDOWS_ALL: Machine = Machine {
        programs: &["wt.exe", "pwsh.exe", "powershell.exe", "cmd.exe"],
        env: &[("COMSPEC", "C:\\Windows\\System32\\cmd.exe")],
    };

    #[test]
    fn windows_prefers_windows_terminal_and_pwsh() {
        let inv = WINDOWS_ALL
            .plan(|p| plan_windows("git status", &config("", "", true), p))
            .unwrap();
        assert_eq!(inv.program, "wt.exe");
        assert_eq!(
            inv.args,
            vec![
                "new-tab",
                "--",
                "pwsh.exe",
                "-NoLogo",
                "-NoExit",
                "-EncodedCommand",
                &powershell_encoded("git status"),
            ]
        );
        assert!(!inv.new_console);
        assert_eq!(inv.raw_tail, None);
    }

    #[test]
    fn windows_falls_back_to_windows_powershell_then_cmd() {
        let machine = Machine {
            programs: &["wt.exe", "powershell.exe"],
            env: &[],
        };
        let inv = machine
            .plan(|p| plan_windows("dir", &config("", "", false), p))
            .unwrap();
        assert_eq!(inv.args[2], "powershell.exe");
        assert!(!inv.args.contains(&"-NoExit".to_owned()));

        let machine = Machine {
            programs: &[],
            env: &[],
        };
        let inv = machine
            .plan(|p| plan_windows("dir", &config("", "", true), p))
            .unwrap();
        assert_eq!(inv.program, "cmd.exe");
        assert!(inv.new_console);
        assert!(inv.args.is_empty());
        assert_eq!(inv.raw_tail.as_deref(), Some(r#"/S /K "dir""#));
    }

    #[test]
    fn windows_without_wt_opens_a_console_window() {
        let machine = Machine {
            programs: &["powershell.exe"],
            env: &[],
        };
        let inv = machine
            .plan(|p| plan_windows("dir", &config("", "", true), p))
            .unwrap();
        assert_eq!(inv.program, "powershell.exe");
        assert!(inv.new_console);
        assert_eq!(inv.args[..3], ["-NoLogo", "-NoExit", "-EncodedCommand"]);
    }

    #[test]
    fn windows_terminal_escapes_semicolons_for_cmd() {
        let inv = WINDOWS_ALL
            .plan(|p| plan_windows(r#"echo a;b "c d""#, &config("", "cmd", true), p))
            .unwrap();
        assert_eq!(inv.program, "wt.exe");
        assert_eq!(
            inv.args,
            vec!["new-tab", "--", "cmd.exe", "/S", "/K", r#"echo a\;b "c d""#]
        );
        assert_eq!(inv.raw_tail, None);
    }

    #[test]
    fn windows_posix_shell_and_custom_terminal() {
        let machine = Machine {
            programs: &["alacritty.exe", "bash.exe"],
            env: &[],
        };
        let inv = machine
            .plan(|p| plan_windows("ls", &config("alacritty --class sevak", "bash", true), p))
            .unwrap();
        assert_eq!(inv.program, "alacritty.exe");
        assert_eq!(
            inv.args,
            vec![
                "--class",
                "sevak",
                "-e",
                "bash.exe",
                "-c",
                "ls\nexec 'bash.exe'"
            ]
        );
    }

    #[test]
    fn windows_conhost_forces_a_console_window() {
        let machine = Machine {
            programs: &["conhost.exe", "wt.exe", "cmd.exe"],
            env: &[],
        };
        let inv = machine
            .plan(|p| plan_windows("dir", &config("conhost", "cmd", false), p))
            .unwrap();
        assert_eq!(inv.program, "cmd.exe");
        assert!(inv.new_console);
        assert_eq!(inv.raw_tail.as_deref(), Some(r#"/S /C "dir""#));
    }

    #[test]
    fn windows_reports_missing_configured_programs() {
        let err = WINDOWS_ALL
            .plan(|p| plan_windows("dir", &config("nope", "", true), p))
            .unwrap_err();
        assert!(err.to_string().contains("`nope`"), "{err}");
        let err = WINDOWS_ALL
            .plan(|p| plan_windows("dir", &config("", "nushell", true), p))
            .unwrap_err();
        assert!(err.to_string().contains("`nushell`"), "{err}");
    }

    #[test]
    fn windows_empty_command_opens_a_plain_terminal() {
        let inv = WINDOWS_ALL
            .plan(|p| plan_windows("", &config("", "", true), p))
            .unwrap();
        assert_eq!(inv.program, "wt.exe");
        assert!(inv.args.is_empty());

        let machine = Machine {
            programs: &["cmd.exe"],
            env: &[],
        };
        let inv = machine
            .plan(|p| plan_windows("", &config("", "", true), p))
            .unwrap();
        assert!(inv.new_console);
        assert!(inv.args.is_empty());
        assert_eq!(inv.raw_tail, None);
    }

    // ----- macOS -----

    #[test]
    fn macos_terminal_app_escapes_the_command() {
        let inv = plan_macos(r#"echo "hi\there" \ x"#, &config("", "", true)).unwrap();
        assert_eq!(inv.program, "osascript");
        assert_eq!(
            inv.args,
            vec![
                "-e",
                "tell application \"Terminal\"",
                "-e",
                r#"do script "echo \"hi\\there\" \\ x""#,
                "-e",
                "activate",
                "-e",
                "end tell",
            ]
        );
    }

    #[test]
    fn macos_exit_is_appended_unless_keeping_the_terminal_open() {
        let inv = plan_macos("make", &config("Terminal.app", "", false)).unwrap();
        assert_eq!(inv.args[3], r#"do script "make; exit""#);
        let inv = plan_macos("make", &config("terminal", "", true)).unwrap();
        assert_eq!(inv.args[3], r#"do script "make""#);
    }

    #[test]
    fn macos_iterm_writes_text_into_a_new_window() {
        for terminal in ["iterm", "iTerm2", "/Applications/iTerm.app"] {
            let inv = plan_macos("top", &config(terminal, "", false)).unwrap();
            assert_eq!(inv.args[1], "tell application \"iTerm\"", "{terminal}");
            assert_eq!(
                inv.args[7],
                r#"tell current session of sevakWindow to write text "top; exit""#
            );
        }
    }

    #[test]
    fn macos_empty_command_and_unsupported_terminals() {
        let inv = plan_macos("", &config("", "", false)).unwrap();
        assert_eq!(inv.args[3], "do script");
        let inv = plan_macos("", &config("iterm", "", true)).unwrap();
        assert_eq!(inv.args.iter().filter(|a| *a == "-e").count(), 4);
        assert!(plan_macos("ls", &config("kitty", "", true)).is_err());
    }

    #[test]
    fn applescript_escapes_control_characters() {
        assert_eq!(applescript_escape("a\nb\tc\rd"), r"a\nb\tc\rd");
    }

    // ----- Linux -----

    fn linux(machine: &Machine, command: &str, config: &ShellConfig) -> Result<Invocation> {
        machine.plan(|p| plan_linux(command, config, p))
    }

    const LINUX_ALL: Machine = Machine {
        programs: &[
            "x-terminal-emulator",
            "gnome-terminal",
            "konsole",
            "kitty",
            "alacritty",
            "wezterm",
            "foot",
            "xterm",
            "zsh",
            "sh",
        ],
        env: &[("SHELL", "zsh")],
    };

    #[test]
    fn linux_detection_order() {
        let order = [
            "x-terminal-emulator",
            "gnome-terminal",
            "konsole",
            "kitty",
            "alacritty",
            "wezterm",
            "foot",
            "xterm",
        ];
        for (i, expected) in order.iter().enumerate() {
            // Only terminals from position `i` onwards are installed.
            let find = |program: &str| order[i..].contains(&program).then(|| program.to_owned());
            let env = |_: &str| None;
            let inv = plan_linux(
                "ls",
                &config("", "", true),
                &Probe {
                    find: &find,
                    env: &env,
                },
            )
            .unwrap();
            assert_eq!(inv.program, *expected);
        }
    }

    #[test]
    fn linux_no_terminal_is_an_error() {
        let machine = Machine {
            programs: &["sh"],
            env: &[],
        };
        let err = linux(&machine, "ls", &config("", "", true)).unwrap_err();
        assert!(err.to_string().contains("no terminal found"), "{err}");
    }

    #[test]
    fn linux_terminal_variable_beats_the_list_and_config_beats_the_variable() {
        let machine = Machine {
            programs: &["xterm", "kitty", "foot", "sh"],
            env: &[("TERMINAL", "kitty")],
        };
        let inv = linux(&machine, "ls", &config("", "", false)).unwrap();
        assert_eq!(inv.program, "kitty");
        let inv = linux(&machine, "ls", &config("foot", "", false)).unwrap();
        assert_eq!(inv.program, "foot");

        // An unusable $TERMINAL falls through to detection.
        let machine = Machine {
            programs: &["xterm", "sh"],
            env: &[("TERMINAL", "gone")],
        };
        let inv = linux(&machine, "ls", &config("", "", false)).unwrap();
        assert_eq!(inv.program, "xterm");
    }

    #[test]
    fn linux_execute_flags_per_terminal() {
        let cases: [(&str, &[&str]); 8] = [
            ("x-terminal-emulator", &["-e", "zsh", "-c", "ls"]),
            ("gnome-terminal", &["--", "zsh", "-c", "ls"]),
            ("konsole", &["-e", "zsh", "-c", "ls"]),
            ("kitty", &["zsh", "-c", "ls"]),
            ("alacritty", &["-e", "zsh", "-c", "ls"]),
            ("wezterm", &["start", "--", "zsh", "-c", "ls"]),
            ("foot", &["zsh", "-c", "ls"]),
            ("xterm", &["-e", "zsh", "-c", "ls"]),
        ];
        for (terminal, expected) in cases {
            let inv = linux(&LINUX_ALL, "ls", &config(terminal, "", false)).unwrap();
            assert_eq!(inv.program, terminal);
            assert_eq!(inv.args, strings(expected), "{terminal}");
        }
    }

    #[test]
    fn linux_other_terminal_families() {
        for (terminal, flag) in [
            ("xfce4-terminal", "-x"),
            ("mate-terminal", "-x"),
            ("tilix", "-e"),
        ] {
            let machine = Machine {
                programs: &["xfce4-terminal", "mate-terminal", "tilix", "sh"],
                env: &[],
            };
            let inv = linux(&machine, "ls", &config(terminal, "", false)).unwrap();
            assert_eq!(inv.args[0], flag, "{terminal}");
        }
    }

    #[test]
    fn linux_keep_open_execs_the_shell_afterwards() {
        let inv = linux(&LINUX_ALL, "make && ./run", &config("xterm", "", true)).unwrap();
        assert_eq!(
            inv.args,
            vec!["-e", "zsh", "-c", "make && ./run\nexec 'zsh'"]
        );
    }

    #[test]
    fn quoting_follows_the_shell_that_would_run() {
        // Only looks the shell up; nothing is started.
        let with = |shell: &str| {
            shell_quoting(&ShellConfig {
                shell: shell.to_owned(),
                ..ShellConfig::default()
            })
        };
        if cfg!(target_os = "macos") {
            // The login shell runs the command, whatever `[shell] shell` says.
            assert_eq!(with("pwsh"), ShellQuoting::Posix);
        } else {
            // Installed or not, the configured shell decides.
            assert_eq!(with("cmd.exe"), ShellQuoting::Cmd);
            assert_eq!(with("pwsh"), ShellQuoting::PowerShell);
            assert_eq!(with("/bin/bash"), ShellQuoting::Posix);
        }
        assert_eq!(ShellQuoting::from(ShellKind::Cmd), ShellQuoting::Cmd);
    }

    #[test]
    fn linux_shell_selection() {
        // Configured shell wins; $SHELL is next; sh is the last resort.
        let machine = Machine {
            programs: &["xterm", "fish", "zsh", "sh"],
            env: &[("SHELL", "zsh")],
        };
        let inv = linux(&machine, "ls", &config("", "fish", false)).unwrap();
        assert_eq!(inv.args[1], "fish");
        let inv = linux(&machine, "ls", &config("", "", false)).unwrap();
        assert_eq!(inv.args[1], "zsh");
        let machine = Machine {
            programs: &["xterm"],
            env: &[("SHELL", "/gone/shell")],
        };
        let inv = linux(&machine, "ls", &config("", "", false)).unwrap();
        assert_eq!(inv.args[1], "sh");
        let err = linux(&machine, "ls", &config("", "nushell", false)).unwrap_err();
        assert!(err.to_string().contains("`nushell`"), "{err}");
    }

    #[test]
    fn linux_configured_terminal_keeps_its_own_arguments() {
        let machine = Machine {
            programs: &["alacritty", "sh"],
            env: &[],
        };
        let inv = linux(
            &machine,
            "ls",
            &config("alacritty --class sevak -o 'a b'", "", false),
        )
        .unwrap();
        assert_eq!(inv.program, "alacritty");
        assert_eq!(
            inv.args,
            vec!["--class", "sevak", "-o", "a b", "-e", "sh", "-c", "ls"]
        );
        let err = linux(&machine, "ls", &config("missing --x", "", false)).unwrap_err();
        assert!(err.to_string().contains("`missing`"), "{err}");
    }

    #[test]
    fn linux_empty_command_opens_just_the_terminal() {
        let inv = linux(&LINUX_ALL, "", &config("kitty --single-instance", "", true)).unwrap();
        assert_eq!(inv.program, "kitty");
        assert_eq!(inv.args, vec!["--single-instance"]);
    }

    #[test]
    fn commands_are_never_split_or_expanded_by_sevak() {
        // Hostile-looking text stays one argv element for the shell to run.
        let command = "echo $(id) `id`; rm -rf ~ && x | y > z";
        let inv = linux(&LINUX_ALL, command, &config("xterm", "", false)).unwrap();
        assert_eq!(inv.args, vec!["-e", "zsh", "-c", command]);
    }

    /// Launches a real terminal; run by hand with
    /// `cargo test -p sevak-platform launches_a_real_terminal -- --ignored`.
    #[test]
    #[ignore = "opens a terminal window"]
    fn launches_a_real_terminal() {
        run_in_terminal("echo hello from sevak", &ShellConfig::default()).unwrap();
    }
}
