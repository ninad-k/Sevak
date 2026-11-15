// The keys the config file accepts in the lists of the plugin sections. Rust
// matches them (`TaskKind::key`, `SystemCommand::key`, `SettingsPage::key`,
// `browsers.rs`); a key that is missing here is still kept when the form saves.

import { t, type MessageKey } from "../i18n.svelte";
import type { KeyOption } from "./types";

/** An option whose label (and group heading) follow the UI language as they are read. */
function option(key: string, label: MessageKey | null, group?: MessageKey, literal = ""): KeyOption {
  return {
    key,
    get label() {
      return label === null ? literal : t(label);
    },
    get group() {
      return group === undefined ? undefined : t(group);
    },
  };
}

/** `[tasks] disabled`: only the tasks that work on this computer are ever offered. */
export const TASK_KEYS: KeyOption[] = [
  option("dark_mode", "pages.keys.tasks.darkMode", "pages.keys.groups.desktop"),
  option("show_desktop", "pages.keys.tasks.showDesktop", "pages.keys.groups.desktop"),
  option("hide_others", "pages.keys.tasks.hideOthers", "pages.keys.groups.desktop"),
  option("minimize_all", "pages.keys.tasks.minimizeAll", "pages.keys.groups.desktop"),
  option("screenshot", "pages.keys.tasks.screenshot", "pages.keys.groups.desktop"),
  option("restart_shell", "pages.keys.tasks.restartShell", "pages.keys.groups.desktop"),
  option("downloads", "pages.keys.tasks.downloads", "pages.keys.groups.filesClipboard"),
  option("recent_files", "pages.keys.tasks.recentFiles", "pages.keys.groups.filesClipboard"),
  option("empty_clipboard", "pages.keys.tasks.emptyClipboard", "pages.keys.groups.filesClipboard"),
  option("mute", "pages.keys.tasks.mute", "pages.keys.groups.sound"),
  option("unmute", "pages.keys.tasks.unmute", "pages.keys.groups.sound"),
  option("volume_up", "pages.keys.tasks.volumeUp", "pages.keys.groups.sound"),
  option("volume_down", "pages.keys.tasks.volumeDown", "pages.keys.groups.sound"),
  option("volume", "pages.keys.tasks.volume", "pages.keys.groups.sound"),
  option("wifi", "pages.keys.tasks.wifi", "pages.keys.groups.network"),
  option("bluetooth", "pages.keys.tasks.bluetooth", "pages.keys.groups.network"),
  option("flush_dns", "pages.keys.tasks.flushDns", "pages.keys.groups.network"),
  option("keep_awake", "pages.keys.tasks.keepAwake", "pages.keys.groups.appsPower"),
  option("stop_keep_awake", "pages.keys.tasks.stopKeepAwake", "pages.keys.groups.appsPower"),
  option("quit_app", "pages.keys.tasks.quitApp", "pages.keys.groups.appsPower"),
  option("force_quit_app", "pages.keys.tasks.forceQuitApp", "pages.keys.groups.appsPower"),
  option("kill", "pages.keys.tasks.kill", "pages.keys.groups.appsPower"),
  option("eject", "pages.keys.tasks.eject", "pages.keys.groups.appsPower"),
];

/** `[system] disabled`: commands, then the settings pages (`settings` hides all of them). */
export const SYSTEM_KEYS: KeyOption[] = [
  option("lock", "pages.keys.system.lock", "pages.keys.groups.commands"),
  option("sleep", "pages.keys.system.sleep", "pages.keys.groups.commands"),
  option("hibernate", "pages.keys.system.hibernate", "pages.keys.groups.commands"),
  option("restart", "pages.keys.system.restart", "pages.keys.groups.commands"),
  option("shutdown", "pages.keys.system.shutdown", "pages.keys.groups.commands"),
  option("logout", "pages.keys.system.logout", "pages.keys.groups.commands"),
  option("empty_trash", "pages.keys.system.emptyTrash", "pages.keys.groups.commands"),
  option("settings", "pages.keys.system.settings", "pages.keys.groups.settingsPages"),
  option("settings:display", "pages.keys.system.display", "pages.keys.groups.settingsPages"),
  option("settings:sound", "pages.keys.system.sound", "pages.keys.groups.settingsPages"),
  option("settings:bluetooth", null, "pages.keys.groups.settingsPages", "Bluetooth"),
  option("settings:network", "pages.keys.system.network", "pages.keys.groups.settingsPages"),
  option("settings:wifi", null, "pages.keys.groups.settingsPages", "Wi-Fi"),
  option("settings:apps", "pages.keys.system.apps", "pages.keys.groups.settingsPages"),
  option("settings:notifications", "pages.keys.system.notifications", "pages.keys.groups.settingsPages"),
  option("settings:power", "pages.keys.system.power", "pages.keys.groups.settingsPages"),
  option("settings:keyboard", "pages.keys.system.keyboard", "pages.keys.groups.settingsPages"),
  option("settings:mouse", "pages.keys.system.mouse", "pages.keys.groups.settingsPages"),
  option("settings:privacy", "pages.keys.system.privacy", "pages.keys.groups.settingsPages"),
  option("settings:datetime", "pages.keys.system.datetime", "pages.keys.groups.settingsPages"),
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
  option("safari", "pages.keys.safari"),
];
