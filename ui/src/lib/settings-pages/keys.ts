// The keys the config file accepts in the lists of the plugin sections. Rust
// matches them (`TaskKind::key`, `SystemCommand::key`, `SettingsPage::key`,
// `browsers.rs`); a key that is missing here is still kept when the form saves.

import type { KeyOption } from "./types";

/** `[tasks] disabled`: only the tasks that work on this computer are ever offered. */
export const TASK_KEYS: KeyOption[] = [
  { key: "dark_mode", label: "Toggle dark mode", group: "Desktop" },
  { key: "show_desktop", label: "Show desktop", group: "Desktop" },
  { key: "hide_others", label: "Hide other apps", group: "Desktop" },
  { key: "minimize_all", label: "Minimize all windows", group: "Desktop" },
  { key: "screenshot", label: "Take a screenshot", group: "Desktop" },
  { key: "restart_shell", label: "Restart Explorer or Finder", group: "Desktop" },
  { key: "downloads", label: "Open Downloads folder", group: "Files and clipboard" },
  { key: "recent_files", label: "Open recent files", group: "Files and clipboard" },
  { key: "empty_clipboard", label: "Empty the clipboard", group: "Files and clipboard" },
  { key: "mute", label: "Mute", group: "Sound" },
  { key: "unmute", label: "Unmute", group: "Sound" },
  { key: "volume_up", label: "Volume up", group: "Sound" },
  { key: "volume_down", label: "Volume down", group: "Sound" },
  { key: "volume", label: "Set the volume", group: "Sound" },
  { key: "wifi", label: "Toggle Wi-Fi", group: "Network" },
  { key: "bluetooth", label: "Toggle Bluetooth", group: "Network" },
  { key: "flush_dns", label: "Flush the DNS cache", group: "Network" },
  { key: "keep_awake", label: "Keep the computer awake", group: "Apps and power" },
  { key: "stop_keep_awake", label: "Stop keeping awake", group: "Apps and power" },
  { key: "quit_app", label: "Quit an app", group: "Apps and power" },
  { key: "force_quit_app", label: "Force quit an app", group: "Apps and power" },
  { key: "kill", label: "End a process (kill)", group: "Apps and power" },
  { key: "eject", label: "Eject a drive", group: "Apps and power" },
];

/** `[system] disabled`: commands, then the settings pages (`settings` hides all of them). */
export const SYSTEM_KEYS: KeyOption[] = [
  { key: "lock", label: "Lock", group: "Commands" },
  { key: "sleep", label: "Sleep", group: "Commands" },
  { key: "hibernate", label: "Hibernate", group: "Commands" },
  { key: "restart", label: "Restart", group: "Commands" },
  { key: "shutdown", label: "Shut down", group: "Commands" },
  { key: "logout", label: "Log out", group: "Commands" },
  { key: "empty_trash", label: "Empty the trash", group: "Commands" },
  { key: "settings", label: "All settings pages", group: "Settings pages" },
  { key: "settings:display", label: "Display", group: "Settings pages" },
  { key: "settings:sound", label: "Sound", group: "Settings pages" },
  { key: "settings:bluetooth", label: "Bluetooth", group: "Settings pages" },
  { key: "settings:network", label: "Network", group: "Settings pages" },
  { key: "settings:wifi", label: "Wi-Fi", group: "Settings pages" },
  { key: "settings:apps", label: "Apps", group: "Settings pages" },
  { key: "settings:notifications", label: "Notifications", group: "Settings pages" },
  { key: "settings:power", label: "Power", group: "Settings pages" },
  { key: "settings:keyboard", label: "Keyboard", group: "Settings pages" },
  { key: "settings:mouse", label: "Mouse", group: "Settings pages" },
  { key: "settings:privacy", label: "Privacy", group: "Settings pages" },
  { key: "settings:datetime", label: "Date and time", group: "Settings pages" },
];

/** `[bookmarks] browsers`: the ids of the browsers Sevak reads bookmarks from. */
export const BROWSER_KEYS: KeyOption[] = [
  { key: "chrome", label: "Chrome" },
  { key: "edge", label: "Edge" },
  { key: "brave", label: "Brave" },
  { key: "vivaldi", label: "Vivaldi" },
  { key: "chromium", label: "Chromium" },
  { key: "opera", label: "Opera" },
  { key: "opera-gx", label: "Opera GX" },
  { key: "firefox", label: "Firefox" },
  { key: "librewolf", label: "LibreWolf" },
  { key: "zen", label: "Zen" },
];
