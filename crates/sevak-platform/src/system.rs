//! Power and session commands (lock, sleep, restart, ...) and the OS settings
//! pages, behind two small closed vocabularies.
//!
//! Both are enums on purpose: the platform layer maps each variant to a fixed
//! command line or settings URI, so no caller can make Sevak run an arbitrary
//! program or open an arbitrary URI scheme. That keeps [`crate::open::open_url`]
//! strict (http, https and mailto only) while still letting the system plugin
//! open `ms-settings:` pages.
//!
//! The per-OS tables ([`windows_shutdown_args`], [`mac_command`],
//! [`linux_command`], ...) are pure functions compiled on every OS, so their
//! unit tests run everywhere; only the dispatchers at the bottom are cfg-gated.

use std::time::Duration;

use crate::error::{PlatformError, Result};

/// A power or session command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemCommand {
    Lock,
    Sleep,
    Hibernate,
    Restart,
    ShutDown,
    LogOut,
    EmptyTrash,
}

impl SystemCommand {
    pub const ALL: [Self; 7] = [
        Self::Lock,
        Self::Sleep,
        Self::Hibernate,
        Self::Restart,
        Self::ShutDown,
        Self::LogOut,
        Self::EmptyTrash,
    ];

    /// Stable identifier used in result ids and `[system] disabled`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Sleep => "sleep",
            Self::Hibernate => "hibernate",
            Self::Restart => "restart",
            Self::ShutDown => "shutdown",
            Self::LogOut => "logout",
            Self::EmptyTrash => "empty_trash",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.key() == key)
    }

    /// Whether the command ends the session or destroys data, so the user
    /// should be asked before it runs.
    pub fn is_destructive(self) -> bool {
        matches!(
            self,
            Self::Restart | Self::ShutDown | Self::LogOut | Self::EmptyTrash
        )
    }
}

/// A page of the operating system's settings app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsPage {
    Display,
    Sound,
    Bluetooth,
    Network,
    Wifi,
    Apps,
    Notifications,
    Power,
    Keyboard,
    Mouse,
    Privacy,
    DateTime,
}

impl SettingsPage {
    pub const ALL: [Self; 12] = [
        Self::Display,
        Self::Sound,
        Self::Bluetooth,
        Self::Network,
        Self::Wifi,
        Self::Apps,
        Self::Notifications,
        Self::Power,
        Self::Keyboard,
        Self::Mouse,
        Self::Privacy,
        Self::DateTime,
    ];

    /// Stable identifier used in result ids and `[system] disabled`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Display => "display",
            Self::Sound => "sound",
            Self::Bluetooth => "bluetooth",
            Self::Network => "network",
            Self::Wifi => "wifi",
            Self::Apps => "apps",
            Self::Notifications => "notifications",
            Self::Power => "power",
            Self::Keyboard => "keyboard",
            Self::Mouse => "mouse",
            Self::Privacy => "privacy",
            Self::DateTime => "datetime",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.key() == key)
    }
}

/// A program and its arguments, never passed through a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandLine {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Windows tables
// ---------------------------------------------------------------------------

/// `shutdown.exe` arguments for the commands it handles. Lock, sleep and the
/// recycle bin use Win32 calls instead.
pub fn windows_shutdown_args(command: SystemCommand) -> Option<&'static [&'static str]> {
    match command {
        SystemCommand::Restart => Some(&["/r", "/t", "0"]),
        SystemCommand::ShutDown => Some(&["/s", "/t", "0"]),
        SystemCommand::LogOut => Some(&["/l"]),
        SystemCommand::Hibernate => Some(&["/h"]),
        SystemCommand::Lock | SystemCommand::Sleep | SystemCommand::EmptyTrash => None,
    }
}

/// The `ms-settings:` URI of a settings page.
pub fn windows_settings_uri(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::Display => "ms-settings:display",
        SettingsPage::Sound => "ms-settings:sound",
        SettingsPage::Bluetooth => "ms-settings:bluetooth",
        SettingsPage::Network => "ms-settings:network",
        SettingsPage::Wifi => "ms-settings:network-wifi",
        SettingsPage::Apps => "ms-settings:appsfeatures",
        SettingsPage::Notifications => "ms-settings:notifications",
        SettingsPage::Power => "ms-settings:powersleep",
        SettingsPage::Keyboard => "ms-settings:typing",
        SettingsPage::Mouse => "ms-settings:mousetouchpad",
        SettingsPage::Privacy => "ms-settings:privacy",
        SettingsPage::DateTime => "ms-settings:dateandtime",
    }
}

// ---------------------------------------------------------------------------
// macOS tables
// ---------------------------------------------------------------------------

const PMSET: &str = "/usr/bin/pmset";
const OSASCRIPT: &str = "/usr/bin/osascript";

/// The command line for a command on macOS; `None` where macOS has no
/// equivalent (hibernate is managed by the system there).
///
/// Lock puts the display to sleep, which locks the session when "Require
/// password after screen saver begins or display is turned off" is set (the
/// default on current macOS). Restart, shut down, log out and empty trash go
/// through Apple events, so the first use makes macOS ask for permission to
/// control System Events or Finder.
pub fn mac_command(command: SystemCommand) -> Option<CommandLine> {
    let tell = |app: &str, what: &str| {
        CommandLine::new(
            OSASCRIPT,
            &["-e", &format!(r#"tell application "{app}" to {what}"#)],
        )
    };
    match command {
        SystemCommand::Lock => Some(CommandLine::new(PMSET, &["displaysleepnow"])),
        SystemCommand::Sleep => Some(CommandLine::new(PMSET, &["sleepnow"])),
        SystemCommand::Hibernate => None,
        SystemCommand::Restart => Some(tell("System Events", "restart")),
        SystemCommand::ShutDown => Some(tell("System Events", "shut down")),
        SystemCommand::LogOut => Some(tell("System Events", "log out")),
        SystemCommand::EmptyTrash => Some(tell("Finder", "empty trash")),
    }
}

/// The `x-apple.systempreferences:` URI of a settings page. The pre-Ventura
/// pane ids are used because current macOS still resolves them and they also
/// work on macOS 11 and 12.
pub fn mac_settings_uri(page: SettingsPage) -> Option<&'static str> {
    match page {
        SettingsPage::Display => Some("x-apple.systempreferences:com.apple.preference.displays"),
        SettingsPage::Sound => Some("x-apple.systempreferences:com.apple.preference.sound"),
        SettingsPage::Bluetooth => {
            Some("x-apple.systempreferences:com.apple.preferences.Bluetooth")
        }
        SettingsPage::Network => Some("x-apple.systempreferences:com.apple.preference.network"),
        SettingsPage::Notifications => {
            Some("x-apple.systempreferences:com.apple.preference.notifications")
        }
        SettingsPage::Power => Some("x-apple.systempreferences:com.apple.preference.battery"),
        SettingsPage::Keyboard => Some("x-apple.systempreferences:com.apple.preference.keyboard"),
        SettingsPage::Mouse => Some("x-apple.systempreferences:com.apple.preference.mouse"),
        SettingsPage::Privacy => Some("x-apple.systempreferences:com.apple.preference.security"),
        SettingsPage::DateTime => Some("x-apple.systempreferences:com.apple.preference.datetime"),
        // Wi-Fi lives inside Network, and there is no Apps pane.
        SettingsPage::Wifi | SettingsPage::Apps => None,
    }
}

// ---------------------------------------------------------------------------
// Linux tables
// ---------------------------------------------------------------------------

/// What the Linux session offers, gathered once by [`LinuxEnv::detect`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinuxEnv {
    /// Which of the helper programs [`LinuxEnv::TOOLS`] are on `PATH`.
    pub tools: Vec<String>,
    /// `XDG_CURRENT_DESKTOP` entries (`ubuntu:GNOME` is `["ubuntu", "GNOME"]`).
    pub desktops: Vec<String>,
    /// `XDG_SESSION_ID`, needed to end a session with `loginctl`.
    pub session_id: Option<String>,
    /// The kernel and swap setup allow hibernation.
    pub can_hibernate: bool,
}

impl LinuxEnv {
    /// Programs the command table may use.
    pub const TOOLS: [&'static str; 8] = [
        "loginctl",
        "systemctl",
        "gio",
        "gnome-session-quit",
        "qdbus6",
        "qdbus",
        "qdbus-qt5",
        "gnome-control-center",
    ];

    pub fn detect() -> Self {
        let swaps = std::fs::read_to_string("/proc/swaps").unwrap_or_default();
        let state = std::fs::read_to_string("/sys/power/state").unwrap_or_default();
        Self {
            tools: Self::TOOLS
                .iter()
                .filter(|tool| crate::process::find_in_path(tool).is_some())
                .map(|tool| (*tool).to_owned())
                .collect(),
            desktops: crate::session::current_desktops(),
            session_id: std::env::var("XDG_SESSION_ID")
                .ok()
                .filter(|id| !id.trim().is_empty()),
            can_hibernate: hibernate_supported(&state, &swaps),
        }
    }

    fn has(&self, tool: &str) -> bool {
        self.tools.iter().any(|t| t == tool)
    }

    fn in_desktop(&self, name: &str) -> bool {
        self.desktops.iter().any(|d| d.eq_ignore_ascii_case(name))
    }
}

/// Hibernation needs the kernel's `disk` sleep state (`/sys/power/state`) and
/// at least one active swap area (`/proc/swaps` has a header plus one line per
/// area).
pub fn hibernate_supported(power_state: &str, swaps: &str) -> bool {
    power_state.split_whitespace().any(|state| state == "disk")
        && swaps.lines().filter(|line| !line.trim().is_empty()).count() > 1
}

/// The command line for a command on this Linux session, or `None` when the
/// tools or desktop it needs are missing.
pub fn linux_command(command: SystemCommand, env: &LinuxEnv) -> Option<CommandLine> {
    match command {
        SystemCommand::Lock => env
            .has("loginctl")
            .then(|| CommandLine::new("loginctl", &["lock-session"])),
        SystemCommand::Sleep => env
            .has("systemctl")
            .then(|| CommandLine::new("systemctl", &["suspend"])),
        SystemCommand::Hibernate => (env.has("systemctl") && env.can_hibernate)
            .then(|| CommandLine::new("systemctl", &["hibernate"])),
        SystemCommand::Restart => env
            .has("systemctl")
            .then(|| CommandLine::new("systemctl", &["reboot"])),
        SystemCommand::ShutDown => env
            .has("systemctl")
            .then(|| CommandLine::new("systemctl", &["poweroff"])),
        SystemCommand::LogOut => linux_logout(env),
        SystemCommand::EmptyTrash => env
            .has("gio")
            .then(|| CommandLine::new("gio", &["trash", "--empty"])),
    }
}

/// Ending a session depends on the desktop: GNOME and KDE each have a
/// supported way that also tells the session manager; elsewhere the session is
/// terminated through logind.
fn linux_logout(env: &LinuxEnv) -> Option<CommandLine> {
    if env.in_desktop("GNOME") && env.has("gnome-session-quit") {
        // `--no-prompt`: Sevak has already asked (or the user turned that off).
        return Some(CommandLine::new(
            "gnome-session-quit",
            &["--logout", "--no-prompt"],
        ));
    }
    if env.in_desktop("KDE") {
        if let Some(qdbus) = ["qdbus6", "qdbus", "qdbus-qt5"]
            .into_iter()
            .find(|tool| env.has(tool))
        {
            return Some(CommandLine::new(
                qdbus,
                &["org.kde.Shutdown", "/Shutdown", "logout"],
            ));
        }
    }
    if env.has("loginctl") {
        let id = env.session_id.as_deref()?;
        return Some(CommandLine::new("loginctl", &["terminate-session", id]));
    }
    None
}

/// The `gnome-control-center` panel name of a settings page.
pub fn linux_settings_panel(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::Display => "display",
        SettingsPage::Sound => "sound",
        SettingsPage::Bluetooth => "bluetooth",
        SettingsPage::Network => "network",
        SettingsPage::Wifi => "wifi",
        SettingsPage::Apps => "applications",
        SettingsPage::Notifications => "notifications",
        SettingsPage::Power => "power",
        SettingsPage::Keyboard => "keyboard",
        SettingsPage::Mouse => "mouse",
        SettingsPage::Privacy => "privacy",
        SettingsPage::DateTime => "datetime",
    }
}

// ---------------------------------------------------------------------------
// Dispatch to the current OS
// ---------------------------------------------------------------------------

/// How long a command may run before Sevak stops waiting for its exit status.
/// Long enough to catch an immediate failure (permission denied, tool missing
/// from the session), short enough that Sevak never hangs on a slow one such
/// as emptying a large trash.
pub(crate) const GRACE: Duration = Duration::from_secs(3);

/// The commands that can work on this system right now. Probes the system
/// (`PATH`, power capabilities), so call it from a background thread.
pub fn supported_commands() -> Vec<SystemCommand> {
    #[cfg(windows)]
    {
        let hibernate = crate::windows::system::can_hibernate();
        SystemCommand::ALL
            .into_iter()
            .filter(|command| *command != SystemCommand::Hibernate || hibernate)
            .collect()
    }
    #[cfg(target_os = "macos")]
    {
        SystemCommand::ALL
            .into_iter()
            .filter(|command| mac_command(*command).is_some())
            .collect()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let env = LinuxEnv::detect();
        SystemCommand::ALL
            .into_iter()
            .filter(|command| linux_command(*command, &env).is_some())
            .collect()
    }
}

/// Runs `command`. Fails fast and with the tool's message when the system
/// refuses it; see [`supported_commands`] for what to offer.
pub fn run_command(command: SystemCommand) -> Result<()> {
    #[cfg(windows)]
    {
        use crate::windows::system as win;
        match command {
            SystemCommand::Lock => win::lock(),
            SystemCommand::Sleep => win::sleep(),
            SystemCommand::EmptyTrash => win::empty_recycle_bin(),
            other => match windows_shutdown_args(other) {
                Some(args) => win::run_shutdown(args),
                None => Err(PlatformError::Unsupported("this system command")),
            },
        }
    }
    #[cfg(target_os = "macos")]
    {
        let line = mac_command(command).ok_or(PlatformError::Unsupported("this system command"))?;
        run_line(&line)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let line = linux_command(command, &LinuxEnv::detect())
            .ok_or(PlatformError::Unsupported("this system command"))?;
        run_line(&line)
    }
}

/// The settings pages that exist on this system.
pub fn supported_settings_pages() -> Vec<SettingsPage> {
    #[cfg(windows)]
    {
        SettingsPage::ALL.to_vec()
    }
    #[cfg(target_os = "macos")]
    {
        SettingsPage::ALL
            .into_iter()
            .filter(|page| mac_settings_uri(*page).is_some())
            .collect()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        if crate::process::find_in_path("gnome-control-center").is_some() {
            SettingsPage::ALL.to_vec()
        } else {
            Vec::new()
        }
    }
}

/// Opens one page of the system's settings app.
pub fn open_settings_page(page: SettingsPage) -> Result<()> {
    #[cfg(windows)]
    {
        crate::windows::shell_execute(
            "open",
            std::ffi::OsStr::new(windows_settings_uri(page)),
            None,
        )?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let uri = mac_settings_uri(page).ok_or(PlatformError::Unsupported("this settings page"))?;
        run_line(&CommandLine::new(crate::macos::OPEN, &[uri]))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // A settings window keeps running, so it is started detached rather
        // than waited on.
        crate::process::spawn_detached("gnome-control-center", &[linux_settings_panel(page)])
    }
}

#[cfg(not(windows))]
fn run_line(line: &CommandLine) -> Result<()> {
    crate::process::run_checked(&line.program, &line.args, GRACE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(tools: &[&str], desktops: &[&str]) -> LinuxEnv {
        LinuxEnv {
            tools: tools.iter().map(|t| (*t).to_owned()).collect(),
            desktops: desktops.iter().map(|d| (*d).to_owned()).collect(),
            session_id: Some("3".to_owned()),
            can_hibernate: true,
        }
    }

    fn line(program: &str, args: &[&str]) -> Option<CommandLine> {
        Some(CommandLine::new(program, args))
    }

    #[test]
    fn keys_are_unique_and_round_trip() {
        for command in SystemCommand::ALL {
            assert_eq!(SystemCommand::from_key(command.key()), Some(command));
        }
        for page in SettingsPage::ALL {
            assert_eq!(SettingsPage::from_key(page.key()), Some(page));
        }
        let mut keys: Vec<_> = SystemCommand::ALL.iter().map(|c| c.key()).collect();
        keys.extend(SettingsPage::ALL.iter().map(|p| p.key()));
        keys.sort_unstable();
        let total = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), total);
        assert_eq!(SystemCommand::from_key("reboot"), None);
    }

    #[test]
    fn only_session_ending_and_data_destroying_commands_are_destructive() {
        let destructive: Vec<_> = SystemCommand::ALL
            .into_iter()
            .filter(|c| c.is_destructive())
            .collect();
        assert_eq!(
            destructive,
            [
                SystemCommand::Restart,
                SystemCommand::ShutDown,
                SystemCommand::LogOut,
                SystemCommand::EmptyTrash
            ]
        );
    }

    #[test]
    fn windows_commands_build_the_documented_shutdown_arguments() {
        assert_eq!(
            windows_shutdown_args(SystemCommand::Restart),
            Some(&["/r", "/t", "0"][..])
        );
        assert_eq!(
            windows_shutdown_args(SystemCommand::ShutDown),
            Some(&["/s", "/t", "0"][..])
        );
        assert_eq!(
            windows_shutdown_args(SystemCommand::LogOut),
            Some(&["/l"][..])
        );
        assert_eq!(
            windows_shutdown_args(SystemCommand::Hibernate),
            Some(&["/h"][..])
        );
        assert_eq!(windows_shutdown_args(SystemCommand::Lock), None);
        assert_eq!(windows_shutdown_args(SystemCommand::Sleep), None);
        assert_eq!(windows_shutdown_args(SystemCommand::EmptyTrash), None);
    }

    #[test]
    fn windows_settings_uris_are_ms_settings() {
        for page in SettingsPage::ALL {
            assert!(windows_settings_uri(page).starts_with("ms-settings:"));
        }
        assert_eq!(
            windows_settings_uri(SettingsPage::Bluetooth),
            "ms-settings:bluetooth"
        );
    }

    #[test]
    fn mac_commands_use_pmset_and_apple_events() {
        assert_eq!(
            mac_command(SystemCommand::Sleep),
            line("/usr/bin/pmset", &["sleepnow"])
        );
        assert_eq!(
            mac_command(SystemCommand::Lock),
            line("/usr/bin/pmset", &["displaysleepnow"])
        );
        assert_eq!(
            mac_command(SystemCommand::Restart),
            line(
                "/usr/bin/osascript",
                &["-e", r#"tell application "System Events" to restart"#]
            )
        );
        assert_eq!(
            mac_command(SystemCommand::ShutDown),
            line(
                "/usr/bin/osascript",
                &["-e", r#"tell application "System Events" to shut down"#]
            )
        );
        assert_eq!(
            mac_command(SystemCommand::LogOut),
            line(
                "/usr/bin/osascript",
                &["-e", r#"tell application "System Events" to log out"#]
            )
        );
        assert_eq!(
            mac_command(SystemCommand::EmptyTrash),
            line(
                "/usr/bin/osascript",
                &["-e", r#"tell application "Finder" to empty trash"#]
            )
        );
        assert_eq!(mac_command(SystemCommand::Hibernate), None);
    }

    #[test]
    fn mac_settings_uris_use_the_system_preferences_scheme() {
        let mut offered = 0;
        for page in SettingsPage::ALL {
            if let Some(uri) = mac_settings_uri(page) {
                assert!(uri.starts_with("x-apple.systempreferences:com.apple."));
                offered += 1;
            }
        }
        assert_eq!(offered, 10);
        assert_eq!(mac_settings_uri(SettingsPage::Apps), None);
    }

    #[test]
    fn linux_power_commands_need_systemd() {
        let systemd = env(&["systemctl", "loginctl"], &[]);
        assert_eq!(
            linux_command(SystemCommand::Sleep, &systemd),
            line("systemctl", &["suspend"])
        );
        assert_eq!(
            linux_command(SystemCommand::Restart, &systemd),
            line("systemctl", &["reboot"])
        );
        assert_eq!(
            linux_command(SystemCommand::ShutDown, &systemd),
            line("systemctl", &["poweroff"])
        );
        assert_eq!(
            linux_command(SystemCommand::Hibernate, &systemd),
            line("systemctl", &["hibernate"])
        );
        assert_eq!(
            linux_command(SystemCommand::Lock, &systemd),
            line("loginctl", &["lock-session"])
        );

        let bare = env(&[], &[]);
        for command in SystemCommand::ALL {
            assert_eq!(linux_command(command, &bare), None, "{command:?}");
        }
    }

    #[test]
    fn linux_hibernate_is_hidden_without_swap_support() {
        let mut no_swap = env(&["systemctl"], &[]);
        no_swap.can_hibernate = false;
        assert_eq!(linux_command(SystemCommand::Hibernate, &no_swap), None);
    }

    #[test]
    fn hibernate_needs_the_disk_state_and_a_swap_area() {
        let swaps = "Filename Type Size Used Priority\n/swapfile file 2097148 0 -2\n";
        assert!(hibernate_supported("freeze mem disk\n", swaps));
        assert!(!hibernate_supported("freeze mem\n", swaps));
        assert!(!hibernate_supported(
            "freeze mem disk\n",
            "Filename Type Size Used Priority\n"
        ));
        assert!(!hibernate_supported("", ""));
    }

    #[test]
    fn linux_empty_trash_uses_gio() {
        assert_eq!(
            linux_command(SystemCommand::EmptyTrash, &env(&["gio"], &[])),
            line("gio", &["trash", "--empty"])
        );
    }

    #[test]
    fn linux_logout_picks_the_session_managers_own_way_out() {
        let gnome = env(&["gnome-session-quit", "loginctl"], &["ubuntu", "GNOME"]);
        assert_eq!(
            linux_command(SystemCommand::LogOut, &gnome),
            line("gnome-session-quit", &["--logout", "--no-prompt"])
        );

        let kde = env(&["qdbus", "qdbus6", "loginctl"], &["KDE"]);
        assert_eq!(
            linux_command(SystemCommand::LogOut, &kde),
            line("qdbus6", &["org.kde.Shutdown", "/Shutdown", "logout"])
        );

        let other = env(&["loginctl"], &["XFCE"]);
        assert_eq!(
            linux_command(SystemCommand::LogOut, &other),
            line("loginctl", &["terminate-session", "3"])
        );
    }

    #[test]
    fn linux_logout_needs_a_session_id_for_logind() {
        let mut other = env(&["loginctl"], &["XFCE"]);
        other.session_id = None;
        assert_eq!(linux_command(SystemCommand::LogOut, &other), None);
        // GNOME's tool on a non-GNOME desktop is not used.
        let wrong = env(&["gnome-session-quit"], &["XFCE"]);
        assert_eq!(linux_command(SystemCommand::LogOut, &wrong), None);
    }

    #[test]
    fn linux_settings_panels_are_plain_names() {
        assert_eq!(linux_settings_panel(SettingsPage::Apps), "applications");
        assert_eq!(linux_settings_panel(SettingsPage::Bluetooth), "bluetooth");
        for page in SettingsPage::ALL {
            let panel = linux_settings_panel(page);
            assert!(!panel.is_empty() && panel.chars().all(|c| c.is_ascii_lowercase()));
        }
    }

    #[test]
    fn this_systems_commands_are_a_subset_of_all() {
        // Smoke test of the real detection; nothing is executed.
        let supported = supported_commands();
        assert!(supported.iter().all(|c| SystemCommand::ALL.contains(c)));
        let pages = supported_settings_pages();
        assert!(pages.iter().all(|p| SettingsPage::ALL.contains(p)));
        #[cfg(windows)]
        {
            assert!(supported.contains(&SystemCommand::Lock));
            assert_eq!(pages.len(), SettingsPage::ALL.len());
        }
    }
}
