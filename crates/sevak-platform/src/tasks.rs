//! Automation tasks: ready-made operating system actions (toggle dark mode,
//! show the desktop, change the volume, quit an app, keep the computer awake,
//! ...), behind a closed vocabulary.
//!
//! Like [`crate::system`], a [`Task`] is an enum on purpose. The platform layer
//! maps every variant to a fixed command line or Win32 call, so no caller can
//! make Sevak run an arbitrary program; the few variants that carry data (a
//! volume, a number of minutes, an app or drive name) validate it in
//! [`Task::from_key`] and again where it is used.
//!
//! What can work differs per OS and per machine (`blueutil` installed or not,
//! a Wi-Fi radio present or not), so [`supported_kinds`] answers that from a
//! [`TaskEnv`] snapshot. The per-OS tables ([`mac_task_command`],
//! [`linux_task_command`], [`windows_task_chord`], ...) and the parsers for
//! tool output are pure functions compiled on every OS, so their unit tests run
//! everywhere; only the dispatchers at the bottom are cfg-gated.

#[cfg(not(windows))]
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use crate::error::{PlatformError, Result};
use crate::system::CommandLine;
#[cfg(not(windows))]
use crate::system::GRACE;

/// Longest `keep awake` the launcher accepts: a day.
pub const MAX_KEEP_AWAKE_MINUTES: u32 = 24 * 60;

/// Which operating system a table is for. Lets one test module cover all three
/// tables on any machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

impl Os {
    /// The OS Sevak was built for.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Linux
        }
    }
}

/// What a task is, without the data it may carry. Used for availability, for
/// the `[tasks] disabled` list and as the first part of a result id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    ToggleDarkMode,
    ShowDesktop,
    HideOthers,
    MinimizeAll,
    Screenshot,
    OpenDownloads,
    OpenRecentFiles,
    FlushDns,
    RestartShell,
    EmptyClipboard,
    Mute,
    Unmute,
    VolumeUp,
    VolumeDown,
    SetVolume,
    ToggleWifi,
    ToggleBluetooth,
    KeepAwake,
    StopKeepAwake,
    QuitApp,
    ForceQuitApp,
    KillProcess,
    Eject,
}

impl TaskKind {
    pub const ALL: [Self; 23] = [
        Self::ToggleDarkMode,
        Self::ShowDesktop,
        Self::HideOthers,
        Self::MinimizeAll,
        Self::Screenshot,
        Self::OpenDownloads,
        Self::OpenRecentFiles,
        Self::FlushDns,
        Self::RestartShell,
        Self::EmptyClipboard,
        Self::Mute,
        Self::Unmute,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::SetVolume,
        Self::ToggleWifi,
        Self::ToggleBluetooth,
        Self::KeepAwake,
        Self::StopKeepAwake,
        Self::QuitApp,
        Self::ForceQuitApp,
        Self::KillProcess,
        Self::Eject,
    ];

    /// Stable identifier used in result ids and `[tasks] disabled`.
    pub fn key(self) -> &'static str {
        match self {
            Self::ToggleDarkMode => "dark_mode",
            Self::ShowDesktop => "show_desktop",
            Self::HideOthers => "hide_others",
            Self::MinimizeAll => "minimize_all",
            Self::Screenshot => "screenshot",
            Self::OpenDownloads => "downloads",
            Self::OpenRecentFiles => "recent_files",
            Self::FlushDns => "flush_dns",
            Self::RestartShell => "restart_shell",
            Self::EmptyClipboard => "empty_clipboard",
            Self::Mute => "mute",
            Self::Unmute => "unmute",
            Self::VolumeUp => "volume_up",
            Self::VolumeDown => "volume_down",
            Self::SetVolume => "volume",
            Self::ToggleWifi => "wifi",
            Self::ToggleBluetooth => "bluetooth",
            Self::KeepAwake => "keep_awake",
            Self::StopKeepAwake => "stop_keep_awake",
            Self::QuitApp => "quit_app",
            Self::ForceQuitApp => "force_quit_app",
            Self::KillProcess => "kill",
            Self::Eject => "eject",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }

    /// Whether the task ends programs or interrupts the desktop, so the user
    /// should be asked before it runs.
    pub fn is_destructive(self) -> bool {
        matches!(
            self,
            Self::ForceQuitApp | Self::KillProcess | Self::RestartShell
        )
    }
}

/// One thing to do, with the data it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    ToggleDarkMode,
    ShowDesktop,
    HideOthers,
    MinimizeAll,
    Screenshot,
    OpenDownloads,
    OpenRecentFiles,
    FlushDns,
    RestartShell,
    EmptyClipboard,
    Mute,
    Unmute,
    VolumeUp,
    VolumeDown,
    /// Output volume in percent, 0 to 100.
    SetVolume(u8),
    ToggleWifi,
    ToggleBluetooth,
    /// Prevent sleep and screen-off for this many minutes.
    KeepAwake(u32),
    StopKeepAwake,
    /// Ask the app with this name (as [`RunningApp::name`] reports it) to quit.
    QuitApp(String),
    ForceQuitApp(String),
    /// End every process with this name (as [`ProcessInfo::name`] reports it).
    KillProcess(String),
    /// Eject or unmount the drive with this id (as [`Drive::id`] reports it).
    Eject(String),
}

impl Task {
    pub fn kind(&self) -> TaskKind {
        match self {
            Self::ToggleDarkMode => TaskKind::ToggleDarkMode,
            Self::ShowDesktop => TaskKind::ShowDesktop,
            Self::HideOthers => TaskKind::HideOthers,
            Self::MinimizeAll => TaskKind::MinimizeAll,
            Self::Screenshot => TaskKind::Screenshot,
            Self::OpenDownloads => TaskKind::OpenDownloads,
            Self::OpenRecentFiles => TaskKind::OpenRecentFiles,
            Self::FlushDns => TaskKind::FlushDns,
            Self::RestartShell => TaskKind::RestartShell,
            Self::EmptyClipboard => TaskKind::EmptyClipboard,
            Self::Mute => TaskKind::Mute,
            Self::Unmute => TaskKind::Unmute,
            Self::VolumeUp => TaskKind::VolumeUp,
            Self::VolumeDown => TaskKind::VolumeDown,
            Self::SetVolume(_) => TaskKind::SetVolume,
            Self::ToggleWifi => TaskKind::ToggleWifi,
            Self::ToggleBluetooth => TaskKind::ToggleBluetooth,
            Self::KeepAwake(_) => TaskKind::KeepAwake,
            Self::StopKeepAwake => TaskKind::StopKeepAwake,
            Self::QuitApp(_) => TaskKind::QuitApp,
            Self::ForceQuitApp(_) => TaskKind::ForceQuitApp,
            Self::KillProcess(_) => TaskKind::KillProcess,
            Self::Eject(_) => TaskKind::Eject,
        }
    }

    /// The task's stable spelling: the kind's key, then `:` and its data
    /// (`dark_mode`, `volume:30`, `quit_app:Slack`). Round-trips through
    /// [`Task::from_key`].
    pub fn to_key(&self) -> String {
        let kind = self.kind().key();
        match self {
            Self::SetVolume(percent) => format!("{kind}:{percent}"),
            Self::KeepAwake(minutes) => format!("{kind}:{minutes}"),
            Self::QuitApp(data)
            | Self::ForceQuitApp(data)
            | Self::KillProcess(data)
            | Self::Eject(data) => format!("{kind}:{data}"),
            _ => kind.to_owned(),
        }
    }

    /// Parses [`Task::to_key`]'s spelling, checking the data: a volume must be
    /// 0 to 100, minutes 1 to [`MAX_KEEP_AWAKE_MINUTES`], names plain text and a
    /// drive one of the shapes [`valid_drive_id`] accepts. `None` for anything
    /// else, so a malformed result id or hotkey target never reaches the OS.
    pub fn from_key(key: &str) -> Option<Self> {
        let (kind, data) = match key.split_once(':') {
            Some((kind, data)) => (TaskKind::from_key(kind)?, Some(data)),
            None => (TaskKind::from_key(key)?, None),
        };
        let name = |data: Option<&str>| data.filter(|name| valid_name(name)).map(str::to_owned);
        match (kind, data) {
            (TaskKind::SetVolume, Some(data)) => parse_digits(data)
                .filter(|v| *v <= 100)
                .map(|v| Self::SetVolume(v as u8)),
            (TaskKind::KeepAwake, Some(data)) => parse_digits(data)
                .filter(|m| (1..=MAX_KEEP_AWAKE_MINUTES).contains(m))
                .map(Self::KeepAwake),
            (TaskKind::QuitApp, data) => name(data).map(Self::QuitApp),
            (TaskKind::ForceQuitApp, data) => name(data).map(Self::ForceQuitApp),
            (TaskKind::KillProcess, data) => name(data).map(Self::KillProcess),
            (TaskKind::Eject, Some(data)) => {
                valid_drive_id(data).then(|| Self::Eject(data.to_owned()))
            }
            (_, Some(_)) => None,
            (TaskKind::ToggleDarkMode, None) => Some(Self::ToggleDarkMode),
            (TaskKind::ShowDesktop, None) => Some(Self::ShowDesktop),
            (TaskKind::HideOthers, None) => Some(Self::HideOthers),
            (TaskKind::MinimizeAll, None) => Some(Self::MinimizeAll),
            (TaskKind::Screenshot, None) => Some(Self::Screenshot),
            (TaskKind::OpenDownloads, None) => Some(Self::OpenDownloads),
            (TaskKind::OpenRecentFiles, None) => Some(Self::OpenRecentFiles),
            (TaskKind::FlushDns, None) => Some(Self::FlushDns),
            (TaskKind::RestartShell, None) => Some(Self::RestartShell),
            (TaskKind::EmptyClipboard, None) => Some(Self::EmptyClipboard),
            (TaskKind::Mute, None) => Some(Self::Mute),
            (TaskKind::Unmute, None) => Some(Self::Unmute),
            (TaskKind::VolumeUp, None) => Some(Self::VolumeUp),
            (TaskKind::VolumeDown, None) => Some(Self::VolumeDown),
            (TaskKind::ToggleWifi, None) => Some(Self::ToggleWifi),
            (TaskKind::ToggleBluetooth, None) => Some(Self::ToggleBluetooth),
            (TaskKind::StopKeepAwake, None) => Some(Self::StopKeepAwake),
            // The kinds that need data, without any.
            (TaskKind::SetVolume | TaskKind::KeepAwake | TaskKind::Eject, None) => None,
        }
    }
}

/// An unsigned decimal number: digits only, so `+5`, `-1` and `0x10` are not
/// numbers here.
fn parse_digits(text: &str) -> Option<u32> {
    (!text.is_empty() && text.len() <= 9 && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// A process or app name: non-empty plain text without path separators or
/// control characters, and not something a command line would read as an option.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 128
        && !name.starts_with('-')
        && !name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
}

/// A drive Sevak may eject: a Windows drive letter (`E:`), a macOS volume
/// (`/Volumes/<name>`) or a Linux block device (`/dev/sdb1`). Nothing else, so
/// the id is safe to put on a command line.
pub fn valid_drive_id(id: &str) -> bool {
    let letter = id.len() == 2 && id.as_bytes()[0].is_ascii_alphabetic() && id.ends_with(':');
    let volume = id
        .strip_prefix("/Volumes/")
        .is_some_and(|name| valid_name(name) && name != ".." && name != ".");
    let device = id.strip_prefix("/dev/").is_some_and(|name| {
        !name.is_empty()
            && name.len() <= 32
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    });
    letter || volume || device
}

/// Names of processes that must never be ended from the launcher: the ones
/// whose loss crashes the session or the machine, and Sevak itself.
const PROTECTED_PROCESSES: [&str; 18] = [
    "system",
    "system idle process",
    "registry",
    "memory compression",
    "smss",
    "csrss",
    "wininit",
    "services",
    "lsass",
    "winlogon",
    "dwm",
    "init",
    "systemd",
    "launchd",
    "kernel_task",
    "windowserver",
    "loginwindow",
    "sevak",
];

/// Whether a process name is on the never-end list (case-insensitive, `.exe`
/// ignored).
pub fn is_protected_process(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    PROTECTED_PROCESSES.contains(&stem)
}

// ---------------------------------------------------------------------------
// Availability
// ---------------------------------------------------------------------------

/// What this machine offers, gathered once by [`TaskEnv::detect`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskEnv {
    /// Which of the helper programs [`TaskEnv::TOOLS`] were found.
    pub tools: Vec<String>,
    /// `XDG_CURRENT_DESKTOP` entries (`ubuntu:GNOME` is `["ubuntu", "GNOME"]`).
    pub desktops: Vec<String>,
    /// The machine has a Wi-Fi radio Sevak can switch (Windows).
    pub wifi_radio: bool,
    /// The machine has a Bluetooth radio Sevak can switch (Windows).
    pub bluetooth_radio: bool,
}

impl TaskEnv {
    /// Programs the tables may use.
    pub const TOOLS: [&'static str; 18] = [
        "gsettings",
        "wmctrl",
        "gnome-screenshot",
        "spectacle",
        "flameshot",
        "xfce4-screenshooter",
        "gio",
        "resolvectl",
        "wpctl",
        "pactl",
        "amixer",
        "nmcli",
        "rfkill",
        "bluetoothctl",
        "blueutil",
        "systemd-inhibit",
        "udisksctl",
        "playerctl",
    ];

    /// Probes the machine (`PATH`, radios). Slow-ish; call it from a background
    /// thread.
    pub fn detect() -> Self {
        let env = Self {
            tools: Self::TOOLS
                .iter()
                .filter(|tool| find_tool(tool).is_some())
                .map(|tool| (*tool).to_owned())
                .collect(),
            desktops: crate::session::current_desktops(),
            ..Self::default()
        };
        #[cfg(windows)]
        let env = {
            let (wifi_radio, bluetooth_radio) = crate::windows::tasks::radio_kinds();
            Self {
                wifi_radio,
                bluetooth_radio,
                ..env
            }
        };
        env
    }

    pub fn has(&self, tool: &str) -> bool {
        self.tools.iter().any(|t| t == tool)
    }

    fn in_desktop(&self, name: &str) -> bool {
        self.desktops.iter().any(|d| d.eq_ignore_ascii_case(name))
    }
}

/// Where Homebrew puts command line tools. A GUI app's `PATH` on macOS lacks
/// them, so they are looked at by absolute path.
const MAC_TOOL_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// The full path of helper program `name`: on `PATH`, or on macOS in the usual
/// Homebrew folders.
pub fn find_tool(name: &str) -> Option<std::path::PathBuf> {
    crate::process::find_in_path(name).or_else(|| {
        if cfg!(target_os = "macos") {
            MAC_TOOL_DIRS
                .iter()
                .map(|dir| std::path::Path::new(dir).join(name))
                .find(|path| path.is_file())
        } else {
            None
        }
    })
}

/// The kinds of task that can work on `os` with the tools `env` found.
///
/// Notes on what is left out, and why:
/// - `FlushDns` needs administrator rights on Windows (`ipconfig /flushdns`)
///   and root on macOS (`killall -HUP mDNSResponder`), so it is only offered
///   on Linux, where `resolvectl` asks through polkit.
/// - `MinimizeAll` and `HideOthers` are Windows and macOS features; Linux
///   desktops have no common way to do them.
pub fn supported_kinds(os: Os, env: &TaskEnv) -> Vec<TaskKind> {
    TaskKind::ALL
        .into_iter()
        .filter(|kind| match os {
            Os::Windows => windows_supports(*kind, env),
            Os::Mac => mac_supports(*kind, env),
            Os::Linux => linux_supports(*kind, env),
        })
        .collect()
}

fn windows_supports(kind: TaskKind, env: &TaskEnv) -> bool {
    match kind {
        TaskKind::FlushDns => false,
        TaskKind::ToggleWifi => env.wifi_radio,
        TaskKind::ToggleBluetooth => env.bluetooth_radio,
        _ => true,
    }
}

fn mac_supports(kind: TaskKind, env: &TaskEnv) -> bool {
    match kind {
        TaskKind::MinimizeAll | TaskKind::OpenRecentFiles | TaskKind::FlushDns => false,
        TaskKind::ToggleBluetooth => env.has("blueutil"),
        _ => true,
    }
}

fn linux_supports(kind: TaskKind, env: &TaskEnv) -> bool {
    match kind {
        TaskKind::ToggleDarkMode => env.has("gsettings") && env.in_desktop("GNOME"),
        TaskKind::ShowDesktop => env.has("wmctrl"),
        TaskKind::Screenshot => linux_screenshot_tool(env).is_some(),
        TaskKind::OpenRecentFiles => env.has("gio"),
        TaskKind::FlushDns => env.has("resolvectl"),
        TaskKind::Mute
        | TaskKind::Unmute
        | TaskKind::VolumeUp
        | TaskKind::VolumeDown
        | TaskKind::SetVolume => linux_volume_tool(env).is_some(),
        TaskKind::ToggleWifi => env.has("nmcli") || env.has("rfkill"),
        TaskKind::ToggleBluetooth => env.has("rfkill") || env.has("bluetoothctl"),
        TaskKind::KeepAwake | TaskKind::StopKeepAwake => env.has("systemd-inhibit"),
        TaskKind::Eject => env.has("udisksctl"),
        TaskKind::HideOthers | TaskKind::MinimizeAll | TaskKind::RestartShell => false,
        TaskKind::OpenDownloads
        | TaskKind::EmptyClipboard
        | TaskKind::QuitApp
        | TaskKind::ForceQuitApp
        | TaskKind::KillProcess => true,
    }
}

// ---------------------------------------------------------------------------
// Windows tables
// ---------------------------------------------------------------------------

/// Windows virtual-key codes used by the chords below.
pub mod vk {
    pub const LWIN: u16 = 0x5B;
    pub const HOME: u16 = 0x24;
    pub const D: u16 = 0x44;
    pub const M: u16 = 0x4D;
    pub const MEDIA_NEXT: u16 = 0xB0;
    pub const MEDIA_PREV: u16 = 0xB1;
    pub const MEDIA_STOP: u16 = 0xB2;
    pub const MEDIA_PLAY_PAUSE: u16 = 0xB3;
}

/// The keys to hold together for the tasks Windows does with a shortcut:
/// Win+D (show desktop), Win+Home (minimize all but the active window) and
/// Win+M (minimize all).
pub fn windows_task_chord(task: &Task) -> Option<&'static [u16]> {
    match task {
        Task::ShowDesktop => Some(&[vk::LWIN, vk::D]),
        Task::HideOthers => Some(&[vk::LWIN, vk::HOME]),
        Task::MinimizeAll => Some(&[vk::LWIN, vk::M]),
        _ => None,
    }
}

/// `ms-screenclip:` opens the Snipping Tool's region capture.
pub const WINDOWS_SCREENSHOT_URI: &str = "ms-screenclip:";
/// Explorer's Recent folder.
pub const WINDOWS_RECENT_TARGET: &str = "shell:recent";

/// PowerShell arguments that eject drive `drive` (a letter such as `E:`) the
/// way Explorer's "Eject" does. The letter is checked, never interpolated
/// blindly.
pub fn windows_eject_args(drive: &str) -> Option<Vec<String>> {
    let id = drive.to_ascii_uppercase();
    (id.len() == 2 && id.as_bytes()[0].is_ascii_uppercase() && id.ends_with(':')).then(|| {
        vec![
            "-NoProfile".to_owned(),
            "-NonInteractive".to_owned(),
            "-Command".to_owned(),
            format!(
                "(New-Object -ComObject Shell.Application).Namespace(17).ParseName('{id}').InvokeVerb('Eject')"
            ),
        ]
    })
}

/// `taskkill` arguments that end Explorer; Windows starts it again by itself.
pub const WINDOWS_KILL_EXPLORER: [&str; 3] = ["/F", "/IM", "explorer.exe"];

// ---------------------------------------------------------------------------
// macOS tables
// ---------------------------------------------------------------------------

const OSASCRIPT: &str = "/usr/bin/osascript";

fn osascript(lines: &[&str]) -> CommandLine {
    let mut args = Vec::with_capacity(lines.len() * 2);
    for line in lines {
        args.push("-e".to_owned());
        args.push((*line).to_owned());
    }
    CommandLine {
        program: OSASCRIPT.to_owned(),
        args,
    }
}

fn line(program: &str, args: &[&str]) -> CommandLine {
    CommandLine {
        program: program.to_owned(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
    }
}

/// The command line for the tasks macOS does with one command. Tasks that need
/// to read state first (Wi-Fi), end processes, or have no macOS equivalent give
/// `None`; see [`mac_wifi_command`] and [`mac_blueutil_args`].
///
/// Dark mode, volume, hiding other apps and quitting an app go through Apple
/// events, so the first use makes macOS ask for permission to control System
/// Events; hiding other apps and showing the desktop press keys and need the
/// Accessibility permission too.
pub fn mac_task_command(task: &Task) -> Option<CommandLine> {
    const SYSTEM_EVENTS: &str = r#"tell application "System Events""#;
    match task {
        Task::ToggleDarkMode => Some(osascript(&[&format!(
            "{SYSTEM_EVENTS} to tell appearance preferences to set dark mode to not dark mode"
        )])),
        // F11 is macOS's default "Show Desktop" shortcut.
        Task::ShowDesktop => Some(osascript(&[&format!("{SYSTEM_EVENTS} to key code 103")])),
        // Option+Command+H hides everything but the frontmost app. The delay
        // lets Sevak's own window go away first, so the app before it is the
        // frontmost one.
        Task::HideOthers => Some(osascript(&[
            "delay 0.4",
            &format!(r#"{SYSTEM_EVENTS} to keystroke "h" using {{command down, option down}}"#),
        ])),
        Task::Screenshot => Some(line("/usr/bin/open", &["-a", "Screenshot"])),
        Task::RestartShell => Some(line("/usr/bin/killall", &["Finder"])),
        Task::Mute => Some(osascript(&["set volume output muted true"])),
        Task::Unmute => Some(osascript(&["set volume output muted false"])),
        Task::VolumeUp => Some(osascript(&[
            "set volume output volume ((output volume of (get volume settings)) + 10)",
        ])),
        Task::VolumeDown => Some(osascript(&[
            "set volume output volume ((output volume of (get volume settings)) - 10)",
        ])),
        Task::SetVolume(percent) if *percent <= 100 => {
            Some(osascript(&[&format!("set volume output volume {percent}")]))
        }
        Task::QuitApp(name) if valid_name(name) => {
            // The name travels as an argument, never inside the script text.
            let mut command = osascript(&[
                "on run argv",
                "tell application (item 1 of argv) to quit",
                "end run",
            ]);
            command.args.push(name.clone());
            Some(command)
        }
        Task::Eject(path) if path.starts_with("/Volumes/") && valid_drive_id(path) => {
            Some(line("/usr/sbin/diskutil", &["eject", path]))
        }
        _ => None,
    }
}

/// `networksetup` listing of the hardware ports, to find the Wi-Fi device.
pub const MAC_LIST_PORTS: (&str, [&str; 1]) = ("/usr/sbin/networksetup", ["-listallhardwareports"]);

/// The Wi-Fi device (`en0`) in `networksetup -listallhardwareports` output.
pub fn parse_wifi_device(listing: &str) -> Option<String> {
    let mut in_wifi = false;
    for text in listing.lines() {
        let text = text.trim();
        if let Some(port) = text.strip_prefix("Hardware Port:") {
            let port = port.trim();
            in_wifi = port.eq_ignore_ascii_case("Wi-Fi") || port.eq_ignore_ascii_case("AirPort");
        } else if in_wifi {
            if let Some(device) = text.strip_prefix("Device:") {
                let device = device.trim();
                let plain = !device.is_empty()
                    && device.len() <= 16
                    && device.bytes().all(|b| b.is_ascii_alphanumeric());
                return plain.then(|| device.to_owned());
            }
        }
    }
    None
}

/// Whether `networksetup -getairportpower` output says the radio is on
/// (`Wi-Fi Power (en0): On`).
pub fn parse_airport_power(output: &str) -> Option<bool> {
    let state = output.trim().rsplit(':').next()?.trim();
    if state.eq_ignore_ascii_case("on") {
        Some(true)
    } else if state.eq_ignore_ascii_case("off") {
        Some(false)
    } else {
        None
    }
}

/// `networksetup` command that gets (`None`) or sets (`Some(on)`) the Wi-Fi
/// power of `device`.
pub fn mac_wifi_command(device: &str, set: Option<bool>) -> Option<CommandLine> {
    let plain = !device.is_empty() && device.bytes().all(|b| b.is_ascii_alphanumeric());
    plain.then(|| match set {
        None => line("/usr/sbin/networksetup", &["-getairportpower", device]),
        Some(on) => line(
            "/usr/sbin/networksetup",
            &["-setairportpower", device, if on { "on" } else { "off" }],
        ),
    })
}

/// `blueutil` arguments that flip Bluetooth power.
pub const MAC_BLUEUTIL_TOGGLE: [&str; 2] = ["--power", "toggle"];

/// Script that lists the GUI apps: one `pid<TAB>name` line each.
pub const MAC_LIST_APPS_SCRIPT: [&str; 7] = [
    r#"tell application "System Events""#,
    r#"set out to """#,
    "repeat with p in (every application process whose background only is false)",
    "set out to out & (unix id of p) & tab & (name of p) & linefeed",
    "end repeat",
    "return out",
    "end tell",
];

/// The apps in [`MAC_LIST_APPS_SCRIPT`]'s output.
pub fn parse_mac_apps(output: &str) -> Vec<(u32, String)> {
    output
        .lines()
        .filter_map(|text| {
            let (pid, name) = text.split_once('\t')?;
            let name = name.trim();
            (valid_name(name)).then_some((pid.trim().parse().ok()?, name.to_owned()))
        })
        .collect()
}

/// `caffeinate` flags: stop the display and the system from idling to sleep,
/// for `-t` seconds.
pub fn mac_caffeinate(minutes: u32) -> CommandLine {
    CommandLine {
        program: "/usr/bin/caffeinate".to_owned(),
        args: vec![
            "-d".to_owned(),
            "-i".to_owned(),
            "-t".to_owned(),
            (u64::from(minutes) * 60).to_string(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Linux tables
// ---------------------------------------------------------------------------

/// The screenshot program to use, best first.
fn linux_screenshot_tool(env: &TaskEnv) -> Option<CommandLine> {
    const TOOLS: [(&str, &[&str]); 4] = [
        ("gnome-screenshot", &["-i"]),
        ("spectacle", &[]),
        ("flameshot", &["gui"]),
        ("xfce4-screenshooter", &[]),
    ];
    TOOLS
        .iter()
        .find(|(tool, _)| env.has(tool))
        .map(|(tool, args)| line(tool, args))
}

/// The volume program to use: PipeWire's `wpctl`, then PulseAudio's `pactl`,
/// then ALSA's `amixer`.
fn linux_volume_tool(env: &TaskEnv) -> Option<&'static str> {
    ["wpctl", "pactl", "amixer"]
        .into_iter()
        .find(|tool| env.has(tool))
}

/// The command line for the tasks Linux does with one command. State-dependent
/// toggles (dark mode, show desktop, Wi-Fi, Bluetooth) use the helpers below;
/// ending processes is done by Sevak itself.
pub fn linux_task_command(task: &Task, env: &TaskEnv) -> Option<CommandLine> {
    if !linux_supports(task.kind(), env) {
        return None;
    }
    match task {
        Task::Screenshot => linux_screenshot_tool(env),
        Task::OpenRecentFiles => Some(line("gio", &["open", "recent:///"])),
        Task::FlushDns => Some(line("resolvectl", &["flush-caches"])),
        Task::Mute | Task::Unmute | Task::VolumeUp | Task::VolumeDown | Task::SetVolume(_) => {
            linux_volume_command(task, linux_volume_tool(env)?)
        }
        Task::Eject(device) if device.starts_with("/dev/") && valid_drive_id(device) => {
            Some(line("udisksctl", &["unmount", "-b", device]))
        }
        _ => None,
    }
}

/// A volume task with `tool` (one of `wpctl`, `pactl`, `amixer`).
pub fn linux_volume_command(task: &Task, tool: &str) -> Option<CommandLine> {
    const SINK: &str = "@DEFAULT_AUDIO_SINK@";
    const PULSE_SINK: &str = "@DEFAULT_SINK@";
    match (tool, task) {
        ("wpctl", Task::Mute) => Some(line("wpctl", &["set-mute", SINK, "1"])),
        ("wpctl", Task::Unmute) => Some(line("wpctl", &["set-mute", SINK, "0"])),
        ("wpctl", Task::VolumeUp) => {
            Some(line("wpctl", &["set-volume", "-l", "1.0", SINK, "10%+"]))
        }
        ("wpctl", Task::VolumeDown) => Some(line("wpctl", &["set-volume", SINK, "10%-"])),
        ("wpctl", Task::SetVolume(p)) if *p <= 100 => {
            Some(line("wpctl", &["set-volume", SINK, &format!("{p}%")]))
        }
        ("pactl", Task::Mute) => Some(line("pactl", &["set-sink-mute", PULSE_SINK, "1"])),
        ("pactl", Task::Unmute) => Some(line("pactl", &["set-sink-mute", PULSE_SINK, "0"])),
        ("pactl", Task::VolumeUp) => Some(line("pactl", &["set-sink-volume", PULSE_SINK, "+10%"])),
        ("pactl", Task::VolumeDown) => {
            Some(line("pactl", &["set-sink-volume", PULSE_SINK, "-10%"]))
        }
        ("pactl", Task::SetVolume(p)) if *p <= 100 => Some(line(
            "pactl",
            &["set-sink-volume", PULSE_SINK, &format!("{p}%")],
        )),
        ("amixer", Task::Mute) => Some(line("amixer", &["-q", "sset", "Master", "mute"])),
        ("amixer", Task::Unmute) => Some(line("amixer", &["-q", "sset", "Master", "unmute"])),
        ("amixer", Task::VolumeUp) => Some(line("amixer", &["-q", "sset", "Master", "10%+"])),
        ("amixer", Task::VolumeDown) => Some(line("amixer", &["-q", "sset", "Master", "10%-"])),
        ("amixer", Task::SetVolume(p)) if *p <= 100 => {
            Some(line("amixer", &["-q", "sset", "Master", &format!("{p}%")]))
        }
        _ => None,
    }
}

/// `gsettings` command that switches GNOME to the dark (`dark`) or light style.
pub fn linux_color_scheme_command(dark: bool) -> CommandLine {
    line(
        "gsettings",
        &[
            "set",
            "org.gnome.desktop.interface",
            "color-scheme",
            if dark { "prefer-dark" } else { "default" },
        ],
    )
}

/// `gsettings get org.gnome.desktop.interface color-scheme`.
pub const LINUX_GET_COLOR_SCHEME: [&str; 3] =
    ["get", "org.gnome.desktop.interface", "color-scheme"];

/// Whether `gsettings get ... color-scheme` output (`'prefer-dark'`) is dark.
pub fn parse_color_scheme_is_dark(output: &str) -> bool {
    output.contains("dark")
}

/// Whether `wmctrl -m` output says the desktop is being shown
/// (`showing the desktop: ON`).
pub fn parse_showing_desktop(output: &str) -> bool {
    output.lines().any(|text| {
        let text = text.trim().to_ascii_lowercase();
        text.starts_with("showing the desktop:") && text.ends_with("on")
    })
}

/// Whether `nmcli radio wifi` output says Wi-Fi is on.
pub fn parse_nm_radio(output: &str) -> Option<bool> {
    match output.trim() {
        "enabled" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

/// Whether `bluetoothctl show` output says the controller is powered.
pub fn parse_bluetoothctl_powered(output: &str) -> Option<bool> {
    output.lines().find_map(|text| {
        let state = text.trim().strip_prefix("Powered:")?.trim();
        match state {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        }
    })
}

/// `systemd-inhibit` holding off idle and sleep for `minutes`.
pub fn linux_inhibit(minutes: u32) -> CommandLine {
    CommandLine {
        program: "systemd-inhibit".to_owned(),
        args: vec![
            "--what=idle:sleep".to_owned(),
            "--who=Sevak".to_owned(),
            "--why=Keep awake".to_owned(),
            "--mode=block".to_owned(),
            "sleep".to_owned(),
            (u64::from(minutes) * 60).to_string(),
        ],
    }
}

/// The removable, mounted volumes in `lsblk -P -o PATH,RM,HOTPLUG,TYPE,MOUNTPOINT,LABEL`
/// output (one `KEY="value"` line per device).
pub fn parse_lsblk(output: &str) -> Vec<Drive> {
    output
        .lines()
        .filter_map(|text| {
            let fields = parse_pairs(text);
            let get = |key: &str| {
                fields
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("")
            };
            let removable = get("RM") == "1" || get("HOTPLUG") == "1";
            let path = get("PATH");
            let mount = get("MOUNTPOINT");
            if !removable || get("TYPE") != "part" || mount.is_empty() || !valid_drive_id(path) {
                return None;
            }
            let label = if get("LABEL").is_empty() {
                mount.rsplit('/').next().unwrap_or(path)
            } else {
                get("LABEL")
            };
            Some(Drive {
                id: path.to_owned(),
                label: label.to_owned(),
            })
        })
        .collect()
}

/// `KEY="value"` pairs of one `lsblk -P` line. Values may contain spaces.
fn parse_pairs(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut rest = text.trim();
    while let Some((key, after)) = rest.split_once("=\"") {
        let Some((value, tail)) = after.split_once('"') else {
            break;
        };
        pairs.push((key.trim().to_owned(), value.to_owned()));
        rest = tail.trim_start();
    }
    pairs
}

// ---------------------------------------------------------------------------
// Live data: processes, apps, drives
// ---------------------------------------------------------------------------

/// A running process, for `kill`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessInfo {
    /// The executable's name as the OS reports it (`chrome.exe`, `firefox`).
    pub name: String,
    pub pid: u32,
    /// Share of the whole machine's CPU, 0 to 100.
    pub cpu_percent: f32,
    /// Resident memory in bytes.
    pub memory_bytes: u64,
}

/// A running app with windows, for `quit`. Grouped by name: an app made of
/// several processes is one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningApp {
    pub name: String,
    pub pids: Vec<u32>,
}

/// A drive that can be ejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    /// What [`Task::Eject`] takes: `E:`, `/Volumes/Backup` or `/dev/sdb1`.
    pub id: String,
    /// What the user knows it as.
    pub label: String,
}

/// Groups `(pid, name)` pairs by app name (case-insensitively), lowest pid
/// first, sorted by name.
pub fn group_apps(pairs: impl IntoIterator<Item = (u32, String)>) -> Vec<RunningApp> {
    let mut apps: Vec<RunningApp> = Vec::new();
    for (pid, name) in pairs {
        match apps
            .iter_mut()
            .find(|app| app.name.eq_ignore_ascii_case(&name))
        {
            Some(app) => {
                if !app.pids.contains(&pid) {
                    app.pids.push(pid);
                }
            }
            None => apps.push(RunningApp {
                name,
                pids: vec![pid],
            }),
        }
    }
    for app in &mut apps {
        app.pids.sort_unstable();
    }
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

/// Takes a sample of every process. Measures CPU use over a short interval, so
/// it takes a quarter of a second: call it from a background thread.
pub fn list_processes() -> Result<Vec<ProcessInfo>> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, MINIMUM_CPU_UPDATE_INTERVAL};

    let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);

    let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f32;
    let own = std::process::id();
    Ok(system
        .processes()
        .iter()
        .filter(|(pid, _)| pid.as_u32() != own)
        .map(|(pid, process)| ProcessInfo {
            name: process.name().to_string_lossy().into_owned(),
            pid: pid.as_u32(),
            cpu_percent: (process.cpu_usage() / cores).clamp(0.0, 100.0),
            memory_bytes: process.memory(),
        })
        .filter(|info| !info.name.is_empty() && !is_protected_process(&info.name))
        .collect())
}

/// The apps that have a window, grouped by name. Probes the system (window
/// list, Apple events): call it from a background thread.
pub fn list_running_apps() -> Result<Vec<RunningApp>> {
    #[cfg(windows)]
    {
        crate::windows::tasks::running_apps()
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = osascript(&MAC_LIST_APPS_SCRIPT);
        let output = capture(&command.program, &std::mem::take(&mut command.args))?;
        Ok(group_apps(parse_mac_apps(&output)))
    }
    #[cfg(target_os = "linux")]
    {
        crate::linux::tasks::running_apps()
    }
}

/// The drives that can be ejected. Probes the system: call it from a
/// background thread.
pub fn list_drives() -> Result<Vec<Drive>> {
    #[cfg(windows)]
    {
        Ok(crate::windows::tasks::removable_drives())
    }
    #[cfg(target_os = "macos")]
    {
        let mut drives = Vec::new();
        for entry in std::fs::read_dir("/Volumes")?.flatten() {
            let path = entry.path();
            // The startup disk is a symlink to `/`; everything else is mounted.
            let is_link = entry.file_type().is_ok_and(|kind| kind.is_symlink());
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let id = format!("/Volumes/{name}");
            if !is_link && valid_drive_id(&id) {
                drives.push(Drive {
                    id,
                    label: name.to_owned(),
                });
            }
        }
        drives.sort_by_key(|drive| drive.label.to_lowercase());
        Ok(drives)
    }
    #[cfg(target_os = "linux")]
    {
        let output = capture(
            "lsblk",
            &["-P", "-o", "PATH,RM,HOTPLUG,TYPE,MOUNTPOINT,LABEL"],
        )?;
        Ok(parse_lsblk(&output))
    }
}

// ---------------------------------------------------------------------------
// Running things
// ---------------------------------------------------------------------------

/// How long a read-only helper may take.
#[cfg(not(windows))]
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs `program` and returns its standard output. Fails when it cannot start,
/// exits non-zero or takes longer than [`CAPTURE_TIMEOUT`]. Never through a
/// shell.
#[cfg(not(windows))]
pub(crate) fn capture<S: AsRef<std::ffi::OsStr>>(program: &str, args: &[S]) -> Result<String> {
    use std::io::Read;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::process::configure_helper_command(&mut command);
    let mut child = command
        .spawn()
        .map_err(|err| PlatformError::CommandFailed {
            command: program.to_owned(),
            message: err.to_string(),
        })?;
    let mut stdout = child.stdout.take();
    // A reader thread, so a program that keeps writing cannot fill the pipe and
    // hang while we wait on it.
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    });
    let deadline = std::time::Instant::now() + CAPTURE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PlatformError::CommandFailed {
                    command: program.to_owned(),
                    message: "timed out".to_owned(),
                });
            }
            Err(err) => {
                return Err(PlatformError::CommandFailed {
                    command: program.to_owned(),
                    message: err.to_string(),
                })
            }
        }
    };
    let text = reader.join().unwrap_or_default();
    if status.success() {
        Ok(text)
    } else {
        Err(PlatformError::CommandFailed {
            command: program.to_owned(),
            message: status.to_string(),
        })
    }
}

#[cfg(not(windows))]
fn run_line(line: &CommandLine) -> Result<()> {
    crate::process::run_checked(&line.program, &line.args, GRACE)
}

/// The helper that keeps the computer awake (macOS and Linux run a program for
/// the duration; Windows holds a power request on a thread).
enum Awake {
    #[cfg(not(windows))]
    Child(Child),
    #[cfg(windows)]
    Thread(std::sync::mpsc::Sender<()>),
}

static AWAKE: Mutex<Option<Awake>> = Mutex::new(None);

fn stop_awake() -> bool {
    let previous = AWAKE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    match previous {
        #[cfg(not(windows))]
        Some(Awake::Child(mut child)) => {
            let _ = child.kill();
            let _ = child.wait();
            true
        }
        #[cfg(windows)]
        Some(Awake::Thread(stop)) => {
            let _ = stop.send(());
            true
        }
        None => false,
    }
}

pub(crate) fn start_awake(minutes: u32, command: Option<CommandLine>) -> Result<()> {
    stop_awake();
    #[cfg(windows)]
    {
        let _ = command;
        let stop = crate::windows::tasks::keep_awake(Duration::from_secs(u64::from(minutes) * 60))?;
        *AWAKE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Awake::Thread(stop));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = minutes;
        let command = command.ok_or(PlatformError::Unsupported("keeping the computer awake"))?;
        let child = Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| PlatformError::CommandFailed {
                command: command.program.clone(),
                message: err.to_string(),
            })?;
        *AWAKE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Awake::Child(child));
        Ok(())
    }
}

/// What this machine can do, for the plugin to offer. Probes the system (`PATH`,
/// radios): call it from a background thread.
pub fn supported_tasks() -> Vec<TaskKind> {
    supported_kinds(Os::current(), &TaskEnv::detect())
}

/// Runs `task`. Fails fast and with the tool's message when the system refuses
/// it; see [`supported_tasks`] for what to offer.
pub fn run_task(task: &Task) -> Result<()> {
    match task {
        Task::OpenDownloads => {
            let dir = dirs::download_dir().ok_or(PlatformError::MissingDirectory("Downloads"))?;
            return crate::open::open_path(&dir);
        }
        Task::EmptyClipboard => return crate::clipboard::clear(),
        Task::StopKeepAwake => {
            stop_awake();
            return Ok(());
        }
        Task::KillProcess(name) => return kill_named(name),
        Task::ForceQuitApp(name) => return force_quit(name),
        _ => {}
    }
    run_platform_task(task)
}

#[cfg(windows)]
fn run_platform_task(task: &Task) -> Result<()> {
    crate::windows::tasks::run(task)
}

#[cfg(target_os = "macos")]
fn run_platform_task(task: &Task) -> Result<()> {
    match task {
        Task::ToggleWifi => {
            let (program, args) = MAC_LIST_PORTS;
            let device = parse_wifi_device(&capture(program, &args)?)
                .ok_or(PlatformError::Unsupported("Wi-Fi on this Mac"))?;
            let get = mac_wifi_command(&device, None).ok_or(PlatformError::Unsupported("Wi-Fi"))?;
            let on = parse_airport_power(&capture(&get.program, &get.args)?).unwrap_or(true);
            let set =
                mac_wifi_command(&device, Some(!on)).ok_or(PlatformError::Unsupported("Wi-Fi"))?;
            run_line(&set)
        }
        Task::ToggleBluetooth => {
            let tool = find_tool("blueutil")
                .ok_or(PlatformError::Unsupported("Bluetooth (install blueutil)"))?;
            crate::process::run_checked(&tool.to_string_lossy(), &MAC_BLUEUTIL_TOGGLE, GRACE)
        }
        Task::KeepAwake(minutes) => start_awake(*minutes, Some(mac_caffeinate(*minutes))),
        Task::QuitApp(name) => {
            let line =
                mac_task_command(task).ok_or(PlatformError::Unsupported("quitting this app"))?;
            let _ = name;
            run_line(&line)
        }
        other => run_line(&mac_task_command(other).ok_or(PlatformError::Unsupported("this task"))?),
    }
}

#[cfg(target_os = "linux")]
fn run_platform_task(task: &Task) -> Result<()> {
    let env = TaskEnv::detect();
    match task {
        Task::ToggleDarkMode => {
            let dark = parse_color_scheme_is_dark(&capture("gsettings", &LINUX_GET_COLOR_SCHEME)?);
            run_line(&linux_color_scheme_command(!dark))
        }
        Task::ShowDesktop => {
            let showing = parse_showing_desktop(&capture("wmctrl", &["-m"])?);
            run_line(&line("wmctrl", &["-k", if showing { "off" } else { "on" }]))
        }
        Task::ToggleWifi => {
            if env.has("nmcli") {
                let on = parse_nm_radio(&capture("nmcli", &["radio", "wifi"])?).unwrap_or(true);
                run_line(&line(
                    "nmcli",
                    &["radio", "wifi", if on { "off" } else { "on" }],
                ))
            } else {
                run_line(&line("rfkill", &["toggle", "wifi"]))
            }
        }
        Task::ToggleBluetooth => {
            if env.has("rfkill") {
                run_line(&line("rfkill", &["toggle", "bluetooth"]))
            } else {
                let on = parse_bluetoothctl_powered(&capture("bluetoothctl", &["show"])?)
                    .unwrap_or(true);
                run_line(&line(
                    "bluetoothctl",
                    &["power", if on { "off" } else { "on" }],
                ))
            }
        }
        Task::KeepAwake(minutes) => start_awake(*minutes, Some(linux_inhibit(*minutes))),
        Task::QuitApp(name) => quit_app(name),
        other => run_line(
            &linux_task_command(other, &env).ok_or(PlatformError::Unsupported("this task"))?,
        ),
    }
}

/// Asks the app to close the way its own Quit does: SIGTERM to its processes.
#[cfg(target_os = "linux")]
fn quit_app(name: &str) -> Result<()> {
    use sysinfo::{Pid, ProcessesToUpdate, Signal, System};

    let app = list_running_apps()?
        .into_iter()
        .find(|app| app.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| not_running(name))?;
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let mut asked = 0;
    for pid in app.pids {
        if let Some(process) = system.process(Pid::from_u32(pid)) {
            if process.kill_with(Signal::Term).unwrap_or(false) {
                asked += 1;
            }
        }
    }
    if asked == 0 {
        return Err(PlatformError::Os {
            operation: "quit",
            message: format!("could not ask {name} to quit"),
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn not_running(name: &str) -> PlatformError {
    PlatformError::Os {
        operation: "quit",
        message: format!("{name} is not running"),
    }
}

/// Ends every process named `name` (case-insensitively), except the protected
/// ones and Sevak. Verifies the names at the moment of ending, so a pid that
/// was reused since the list was shown is never hit.
fn kill_named(name: &str) -> Result<()> {
    if !valid_name(name) || is_protected_process(name) {
        return Err(PlatformError::Os {
            operation: "end process",
            message: format!("{name} cannot be ended from Sevak"),
        });
    }
    kill_matching(name, &[])
}

/// Force quit: the app's own processes (window owners) and every process of
/// that name.
fn force_quit(name: &str) -> Result<()> {
    if !valid_name(name) || is_protected_process(name) {
        return Err(PlatformError::Os {
            operation: "force quit",
            message: format!("{name} cannot be ended from Sevak"),
        });
    }
    let extra = list_running_apps()
        .ok()
        .and_then(|apps| {
            apps.into_iter()
                .find(|app| app.name.eq_ignore_ascii_case(name))
                .map(|app| app.pids)
        })
        .unwrap_or_default();
    kill_matching(name, &extra)
}

fn kill_matching(name: &str, extra_pids: &[u32]) -> Result<()> {
    use sysinfo::{ProcessesToUpdate, System};

    let own = std::process::id();
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let mut found = 0;
    let mut failed = 0;
    for (pid, process) in system.processes() {
        let same = process.name().to_string_lossy().eq_ignore_ascii_case(name);
        if pid.as_u32() == own || !(same || extra_pids.contains(&pid.as_u32())) {
            continue;
        }
        found += 1;
        if !process.kill() {
            failed += 1;
        }
    }
    if found == 0 {
        return Err(PlatformError::Os {
            operation: "end process",
            message: format!("{name} is not running"),
        });
    }
    if failed == found {
        return Err(PlatformError::Os {
            operation: "end process",
            message: format!("{name} refused to end (it may belong to another user or need administrator rights)"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(tools: &[&str], desktops: &[&str]) -> TaskEnv {
        TaskEnv {
            tools: tools.iter().map(|t| (*t).to_owned()).collect(),
            desktops: desktops.iter().map(|d| (*d).to_owned()).collect(),
            ..TaskEnv::default()
        }
    }

    fn cmd(program: &str, args: &[&str]) -> Option<CommandLine> {
        Some(line(program, args))
    }

    #[test]
    fn kind_keys_are_unique_and_round_trip() {
        let mut keys: Vec<_> = TaskKind::ALL.iter().map(|k| k.key()).collect();
        for kind in TaskKind::ALL {
            assert_eq!(TaskKind::from_key(kind.key()), Some(kind));
        }
        keys.sort_unstable();
        let total = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), total);
        assert_eq!(TaskKind::from_key("reboot"), None);
    }

    #[test]
    fn only_process_ending_and_shell_restarting_tasks_are_destructive() {
        let destructive: Vec<_> = TaskKind::ALL
            .into_iter()
            .filter(|kind| kind.is_destructive())
            .collect();
        assert_eq!(
            destructive,
            [
                TaskKind::RestartShell,
                TaskKind::ForceQuitApp,
                TaskKind::KillProcess
            ]
        );
    }

    #[test]
    fn task_keys_round_trip_with_their_data() {
        let tasks = [
            Task::ToggleDarkMode,
            Task::Mute,
            Task::SetVolume(0),
            Task::SetVolume(30),
            Task::SetVolume(100),
            Task::KeepAwake(1),
            Task::KeepAwake(MAX_KEEP_AWAKE_MINUTES),
            Task::QuitApp("Slack".into()),
            Task::ForceQuitApp("Google Chrome".into()),
            Task::KillProcess("chrome.exe".into()),
            Task::Eject("E:".into()),
            Task::Eject("/Volumes/My Backup".into()),
            Task::Eject("/dev/sdb1".into()),
        ];
        for task in tasks {
            assert_eq!(
                Task::from_key(&task.to_key()).as_ref(),
                Some(&task),
                "{task:?}"
            );
        }
        assert_eq!(Task::SetVolume(30).to_key(), "volume:30");
        assert_eq!(Task::QuitApp("Slack".into()).to_key(), "quit_app:Slack");
        // Data may itself contain a colon.
        assert_eq!(
            Task::from_key("quit_app:a:b"),
            Some(Task::QuitApp("a:b".into()))
        );
        // Every parameterless kind has a bare key; every other needs its data.
        for kind in TaskKind::ALL {
            let parsed = Task::from_key(kind.key());
            let needs_data = matches!(
                kind,
                TaskKind::SetVolume
                    | TaskKind::KeepAwake
                    | TaskKind::QuitApp
                    | TaskKind::ForceQuitApp
                    | TaskKind::KillProcess
                    | TaskKind::Eject
            );
            assert_eq!(parsed.is_some(), !needs_data, "{kind:?}");
        }
    }

    #[test]
    fn malformed_task_keys_are_rejected() {
        for key in [
            "",
            "nope",
            "volume",
            "volume:",
            "volume:101",
            "volume:-1",
            "volume:+5",
            "volume:3x",
            "volume:99999999999",
            "keep_awake:0",
            "keep_awake:1441",
            "keep_awake:abc",
            "dark_mode:1",
            "quit_app",
            "quit_app:",
            "quit_app:-flag",
            "quit_app:a/b",
            "kill:..\\x",
            "kill:line\nbreak",
            "eject",
            "eject:C",
            "eject:/etc",
            "eject:/Volumes/../etc",
            "eject:/dev/",
            "eject:/dev/sda1;rm",
            "eject:E:\\",
        ] {
            assert_eq!(Task::from_key(key), None, "{key:?}");
        }
    }

    #[test]
    fn drive_ids_are_limited_to_known_shapes() {
        for good in [
            "E:",
            "e:",
            "/Volumes/USB",
            "/Volumes/My Disk",
            "/dev/sdb1",
            "/dev/nvme0n1p2",
            "/dev/mmcblk0p1",
        ] {
            assert!(valid_drive_id(good), "{good}");
        }
        for bad in [
            "",
            "E",
            "EE:",
            "1:",
            "/",
            "/Volumes",
            "/Volumes/",
            "/Volumes/..",
            "/dev",
            "/dev/a b",
            "/dev/a/b",
            "C:\\",
            "/tmp/x",
        ] {
            assert!(!valid_drive_id(bad), "{bad}");
        }
    }

    #[test]
    fn protected_processes_cannot_be_targeted() {
        for name in [
            "csrss.exe",
            "CSRSS",
            "System",
            "wininit.exe",
            "launchd",
            "systemd",
            "Sevak",
            "sevak.exe",
            "WindowServer",
        ] {
            assert!(is_protected_process(name), "{name}");
        }
        for name in ["chrome.exe", "explorer.exe", "firefox", "Slack"] {
            assert!(!is_protected_process(name), "{name}");
        }
        assert!(matches!(
            run_task(&Task::KillProcess("csrss.exe".into())),
            Err(PlatformError::Os { .. })
        ));
        assert!(matches!(
            run_task(&Task::ForceQuitApp("Sevak".into())),
            Err(PlatformError::Os { .. })
        ));
    }

    #[test]
    fn availability_tables_per_os() {
        let none = TaskEnv::default();

        let windows = supported_kinds(Os::Windows, &none);
        assert!(windows.contains(&TaskKind::ToggleDarkMode));
        assert!(windows.contains(&TaskKind::MinimizeAll));
        assert!(windows.contains(&TaskKind::Eject));
        // Needs administrator rights; radios not detected.
        assert!(!windows.contains(&TaskKind::FlushDns));
        assert!(!windows.contains(&TaskKind::ToggleWifi));
        assert!(!windows.contains(&TaskKind::ToggleBluetooth));
        let radios = TaskEnv {
            wifi_radio: true,
            bluetooth_radio: true,
            ..TaskEnv::default()
        };
        let windows = supported_kinds(Os::Windows, &radios);
        assert!(
            windows.contains(&TaskKind::ToggleWifi) && windows.contains(&TaskKind::ToggleBluetooth)
        );

        let mac = supported_kinds(Os::Mac, &none);
        assert!(mac.contains(&TaskKind::HideOthers));
        assert!(mac.contains(&TaskKind::ToggleWifi));
        assert!(!mac.contains(&TaskKind::ToggleBluetooth));
        assert!(!mac.contains(&TaskKind::MinimizeAll));
        assert!(!mac.contains(&TaskKind::FlushDns));
        assert!(
            supported_kinds(Os::Mac, &env(&["blueutil"], &[])).contains(&TaskKind::ToggleBluetooth)
        );

        // A bare Linux box can still end processes, open Downloads and clear
        // the clipboard, and nothing else.
        let linux = supported_kinds(Os::Linux, &none);
        assert_eq!(
            linux,
            [
                TaskKind::OpenDownloads,
                TaskKind::EmptyClipboard,
                TaskKind::QuitApp,
                TaskKind::ForceQuitApp,
                TaskKind::KillProcess
            ]
        );
    }

    #[test]
    fn linux_availability_follows_the_tools_and_desktop() {
        let kinds =
            |tools: &[&str], desktops: &[&str]| supported_kinds(Os::Linux, &env(tools, desktops));
        // Dark mode needs GNOME's settings, not just the tool.
        assert!(!kinds(&["gsettings"], &["KDE"]).contains(&TaskKind::ToggleDarkMode));
        assert!(kinds(&["gsettings"], &["ubuntu", "GNOME"]).contains(&TaskKind::ToggleDarkMode));
        assert!(kinds(&["wmctrl"], &[]).contains(&TaskKind::ShowDesktop));
        assert!(kinds(&["spectacle"], &[]).contains(&TaskKind::Screenshot));
        assert!(!kinds(&[], &[]).contains(&TaskKind::Screenshot));
        for tool in ["wpctl", "pactl", "amixer"] {
            let kinds = kinds(&[tool], &[]);
            for kind in [
                TaskKind::Mute,
                TaskKind::Unmute,
                TaskKind::VolumeUp,
                TaskKind::VolumeDown,
                TaskKind::SetVolume,
            ] {
                assert!(kinds.contains(&kind), "{tool} {kind:?}");
            }
        }
        assert!(kinds(&["nmcli"], &[]).contains(&TaskKind::ToggleWifi));
        assert!(kinds(&["rfkill"], &[]).contains(&TaskKind::ToggleWifi));
        assert!(kinds(&["rfkill"], &[]).contains(&TaskKind::ToggleBluetooth));
        assert!(kinds(&["bluetoothctl"], &[]).contains(&TaskKind::ToggleBluetooth));
        assert!(kinds(&["resolvectl"], &[]).contains(&TaskKind::FlushDns));
        assert!(kinds(&["systemd-inhibit"], &[]).contains(&TaskKind::KeepAwake));
        assert!(kinds(&["udisksctl"], &[]).contains(&TaskKind::Eject));
        assert!(kinds(&["gio"], &[]).contains(&TaskKind::OpenRecentFiles));
    }

    #[test]
    fn windows_chords_hold_the_documented_keys() {
        assert_eq!(
            windows_task_chord(&Task::ShowDesktop),
            Some(&[0x5B, 0x44][..])
        );
        assert_eq!(
            windows_task_chord(&Task::HideOthers),
            Some(&[0x5B, 0x24][..])
        );
        assert_eq!(
            windows_task_chord(&Task::MinimizeAll),
            Some(&[0x5B, 0x4D][..])
        );
        assert_eq!(windows_task_chord(&Task::Mute), None);
    }

    #[test]
    fn windows_eject_only_accepts_a_drive_letter() {
        let args = windows_eject_args("e:").unwrap();
        assert_eq!(args[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
        assert!(args[3].contains("ParseName('E:')"), "{}", args[3]);
        assert!(args[3].contains("InvokeVerb('Eject')"));
        for bad in ["", "E", "E:\\", "'; rm", "/dev/sda1", "1:"] {
            assert_eq!(windows_eject_args(bad), None, "{bad}");
        }
    }

    #[test]
    fn mac_commands_use_apple_events_and_fixed_tools() {
        assert_eq!(
            mac_task_command(&Task::ToggleDarkMode),
            cmd(
                "/usr/bin/osascript",
                &[
                    "-e",
                    r#"tell application "System Events" to tell appearance preferences to set dark mode to not dark mode"#
                ]
            )
        );
        assert_eq!(
            mac_task_command(&Task::SetVolume(30)),
            cmd("/usr/bin/osascript", &["-e", "set volume output volume 30"])
        );
        assert_eq!(
            mac_task_command(&Task::Mute),
            cmd(
                "/usr/bin/osascript",
                &["-e", "set volume output muted true"]
            )
        );
        assert_eq!(
            mac_task_command(&Task::Unmute),
            cmd(
                "/usr/bin/osascript",
                &["-e", "set volume output muted false"]
            )
        );
        let up = mac_task_command(&Task::VolumeUp).unwrap();
        assert!(up.args[1].ends_with("+ 10)"), "{:?}", up.args);
        let down = mac_task_command(&Task::VolumeDown).unwrap();
        assert!(down.args[1].ends_with("- 10)"), "{:?}", down.args);
        assert_eq!(mac_task_command(&Task::SetVolume(101)), None);
        assert_eq!(
            mac_task_command(&Task::RestartShell),
            cmd("/usr/bin/killall", &["Finder"])
        );
        assert_eq!(
            mac_task_command(&Task::Screenshot),
            cmd("/usr/bin/open", &["-a", "Screenshot"])
        );
        let hide = mac_task_command(&Task::HideOthers).unwrap();
        assert!(hide.args.iter().any(|a| a.contains("keystroke \"h\"")));
        assert!(mac_task_command(&Task::ShowDesktop).unwrap().args[1].ends_with("key code 103"));
        assert_eq!(
            mac_task_command(&Task::Eject("/Volumes/Backup".into())),
            cmd("/usr/sbin/diskutil", &["eject", "/Volumes/Backup"])
        );
        assert_eq!(mac_task_command(&Task::Eject("/dev/sda1".into())), None);
        // Needs state, or Sevak does it itself.
        for task in [
            Task::ToggleWifi,
            Task::ToggleBluetooth,
            Task::MinimizeAll,
            Task::KillProcess("x".into()),
        ] {
            assert_eq!(mac_task_command(&task), None, "{task:?}");
        }
    }

    #[test]
    fn mac_quit_passes_the_app_name_as_an_argument_not_script_text() {
        let evil = r#"Safari" to do shell script "rm -rf ~""#;
        let command = mac_task_command(&Task::QuitApp(evil.into())).unwrap();
        assert_eq!(command.program, "/usr/bin/osascript");
        // The script lines never contain the name; it is the last argument.
        assert_eq!(command.args.last().map(String::as_str), Some(evil));
        let script: Vec<_> = command.args.iter().filter(|a| *a != "-e").take(3).collect();
        assert!(script.iter().all(|line| !line.contains("rm -rf")));
        assert_eq!(
            command.args[..6],
            [
                "-e",
                "on run argv",
                "-e",
                "tell application (item 1 of argv) to quit",
                "-e",
                "end run"
            ]
        );
        // A name that could pass for an option is refused.
        assert_eq!(mac_task_command(&Task::QuitApp("-x".into())), None);
    }

    #[test]
    fn mac_wifi_parsing_and_commands() {
        let listing = "Hardware Port: Ethernet\nDevice: en5\nEthernet Address: aa\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: bb\n\nHardware Port: Thunderbolt 1\nDevice: en1\n";
        assert_eq!(parse_wifi_device(listing), Some("en0".to_owned()));
        assert_eq!(
            parse_wifi_device("Hardware Port: Ethernet\nDevice: en5\n"),
            None
        );
        assert_eq!(parse_wifi_device(""), None);
        assert_eq!(parse_airport_power("Wi-Fi Power (en0): On\n"), Some(true));
        assert_eq!(parse_airport_power("Wi-Fi Power (en0): Off"), Some(false));
        assert_eq!(parse_airport_power("garbage"), None);
        assert_eq!(
            mac_wifi_command("en0", None),
            cmd("/usr/sbin/networksetup", &["-getairportpower", "en0"])
        );
        assert_eq!(
            mac_wifi_command("en0", Some(false)),
            cmd(
                "/usr/sbin/networksetup",
                &["-setairportpower", "en0", "off"]
            )
        );
        assert_eq!(mac_wifi_command("en0; reboot", Some(true)), None);
        assert_eq!(mac_wifi_command("", None), None);
    }

    #[test]
    fn mac_app_list_parsing() {
        let output = "501\tFinder\n1234\tGoogle Chrome\nbad line\nx\tNope\n77\t\n";
        assert_eq!(
            parse_mac_apps(output),
            [
                (501, "Finder".to_owned()),
                (1234, "Google Chrome".to_owned())
            ]
        );
    }

    #[test]
    fn caffeinate_and_inhibit_use_seconds() {
        assert_eq!(
            mac_caffeinate(30),
            line("/usr/bin/caffeinate", &["-d", "-i", "-t", "1800"])
        );
        let inhibit = linux_inhibit(2);
        assert_eq!(inhibit.program, "systemd-inhibit");
        assert_eq!(inhibit.args[inhibit.args.len() - 2..], ["sleep", "120"]);
        assert!(inhibit.args.contains(&"--what=idle:sleep".to_owned()));
        // The largest allowed value does not overflow.
        assert_eq!(mac_caffeinate(MAX_KEEP_AWAKE_MINUTES).args[3], "86400");
    }

    #[test]
    fn linux_volume_commands_per_tool() {
        let set = Task::SetVolume(30);
        assert_eq!(
            linux_volume_command(&set, "wpctl"),
            cmd("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", "30%"])
        );
        assert_eq!(
            linux_volume_command(&set, "pactl"),
            cmd("pactl", &["set-sink-volume", "@DEFAULT_SINK@", "30%"])
        );
        assert_eq!(
            linux_volume_command(&set, "amixer"),
            cmd("amixer", &["-q", "sset", "Master", "30%"])
        );
        assert_eq!(
            linux_volume_command(&Task::Mute, "wpctl"),
            cmd("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "1"])
        );
        assert_eq!(
            linux_volume_command(&Task::Unmute, "pactl"),
            cmd("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "0"])
        );
        assert_eq!(
            linux_volume_command(&Task::VolumeUp, "wpctl"),
            cmd(
                "wpctl",
                &["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "10%+"]
            )
        );
        assert_eq!(
            linux_volume_command(&Task::VolumeDown, "amixer"),
            cmd("amixer", &["-q", "sset", "Master", "10%-"])
        );
        assert_eq!(linux_volume_command(&Task::SetVolume(150), "wpctl"), None);
        assert_eq!(linux_volume_command(&set, "rm"), None);
    }

    #[test]
    fn linux_tool_choice_prefers_pipewire_then_pulse_then_alsa() {
        let task = Task::VolumeUp;
        let program =
            |tools: &[&str]| linux_task_command(&task, &env(tools, &[])).map(|c| c.program);
        assert_eq!(
            program(&["amixer", "pactl", "wpctl"]).as_deref(),
            Some("wpctl")
        );
        assert_eq!(program(&["amixer", "pactl"]).as_deref(), Some("pactl"));
        assert_eq!(program(&["amixer"]).as_deref(), Some("amixer"));
        assert_eq!(program(&[]), None);
    }

    #[test]
    fn linux_other_commands() {
        let all = env(
            &[
                "gnome-screenshot",
                "spectacle",
                "gio",
                "resolvectl",
                "udisksctl",
            ],
            &["GNOME"],
        );
        assert_eq!(
            linux_task_command(&Task::Screenshot, &all),
            cmd("gnome-screenshot", &["-i"])
        );
        assert_eq!(
            linux_task_command(&Task::Screenshot, &env(&["spectacle"], &[])),
            cmd("spectacle", &[])
        );
        assert_eq!(
            linux_task_command(&Task::OpenRecentFiles, &all),
            cmd("gio", &["open", "recent:///"])
        );
        assert_eq!(
            linux_task_command(&Task::FlushDns, &all),
            cmd("resolvectl", &["flush-caches"])
        );
        assert_eq!(
            linux_task_command(&Task::Eject("/dev/sdb1".into()), &all),
            cmd("udisksctl", &["unmount", "-b", "/dev/sdb1"])
        );
        assert_eq!(linux_task_command(&Task::Eject("E:".into()), &all), None);
        assert_eq!(linux_task_command(&Task::FlushDns, &env(&[], &[])), None);
    }

    #[test]
    fn linux_state_parsers() {
        assert!(parse_color_scheme_is_dark("'prefer-dark'\n"));
        assert!(!parse_color_scheme_is_dark("'default'\n"));
        assert!(!parse_color_scheme_is_dark("'prefer-light'"));
        assert_eq!(
            linux_color_scheme_command(true).args,
            [
                "set",
                "org.gnome.desktop.interface",
                "color-scheme",
                "prefer-dark"
            ]
        );
        assert_eq!(linux_color_scheme_command(false).args[3], "default");
        assert!(parse_showing_desktop(
            "Name: GNOME Shell\nshowing the desktop: ON\n"
        ));
        assert!(!parse_showing_desktop("showing the desktop: OFF\n"));
        assert!(!parse_showing_desktop(""));
        assert_eq!(parse_nm_radio("enabled\n"), Some(true));
        assert_eq!(parse_nm_radio("disabled"), Some(false));
        assert_eq!(parse_nm_radio("?"), None);
        assert_eq!(
            parse_bluetoothctl_powered("Controller AA\n\tName: x\n\tPowered: yes\n"),
            Some(true)
        );
        assert_eq!(parse_bluetoothctl_powered("\tPowered: no"), Some(false));
        assert_eq!(parse_bluetoothctl_powered("No default controller"), None);
    }

    #[test]
    fn lsblk_lists_only_mounted_removable_partitions() {
        let output = concat!(
            "PATH=\"/dev/sda\" RM=\"0\" HOTPLUG=\"0\" TYPE=\"disk\" MOUNTPOINT=\"\" LABEL=\"\"\n",
            "PATH=\"/dev/sda1\" RM=\"0\" HOTPLUG=\"0\" TYPE=\"part\" MOUNTPOINT=\"/\" LABEL=\"root\"\n",
            "PATH=\"/dev/sdb\" RM=\"1\" HOTPLUG=\"1\" TYPE=\"disk\" MOUNTPOINT=\"\" LABEL=\"\"\n",
            "PATH=\"/dev/sdb1\" RM=\"1\" HOTPLUG=\"1\" TYPE=\"part\" MOUNTPOINT=\"/run/media/u/MY STICK\" LABEL=\"MY STICK\"\n",
            "PATH=\"/dev/sdc1\" RM=\"0\" HOTPLUG=\"1\" TYPE=\"part\" MOUNTPOINT=\"/run/media/u/disk\" LABEL=\"\"\n",
            "PATH=\"/dev/sdd1\" RM=\"1\" HOTPLUG=\"1\" TYPE=\"part\" MOUNTPOINT=\"\" LABEL=\"unmounted\"\n",
        );
        assert_eq!(
            parse_lsblk(output),
            [
                Drive {
                    id: "/dev/sdb1".into(),
                    label: "MY STICK".into()
                },
                Drive {
                    id: "/dev/sdc1".into(),
                    label: "disk".into()
                }
            ]
        );
        assert!(parse_lsblk("").is_empty());
    }

    #[test]
    fn apps_are_grouped_by_name_case_insensitively() {
        let apps = group_apps([
            (30, "notepad.exe".to_owned()),
            (10, "Chrome.exe".to_owned()),
            (20, "chrome.exe".to_owned()),
            (10, "chrome.exe".to_owned()),
        ]);
        assert_eq!(
            apps,
            [
                RunningApp {
                    name: "Chrome.exe".into(),
                    pids: vec![10, 20]
                },
                RunningApp {
                    name: "notepad.exe".into(),
                    pids: vec![30]
                }
            ]
        );
    }

    #[test]
    fn this_machines_tasks_are_a_subset_of_all() {
        // Smoke test of the real detection; nothing is executed.
        let supported = supported_tasks();
        assert!(supported.iter().all(|kind| TaskKind::ALL.contains(kind)));
        assert!(supported.contains(&TaskKind::KillProcess));
    }

    #[test]
    fn listing_processes_finds_this_test_binary() {
        let processes = list_processes().expect("process list");
        assert!(processes.iter().any(|p| p.pid != 0 && !p.name.is_empty()));
        assert!(processes
            .iter()
            .all(|p| (0.0..=100.0).contains(&p.cpu_percent)));
        // Sevak never offers itself.
        assert!(processes.iter().all(|p| p.pid != std::process::id()));
    }
}
