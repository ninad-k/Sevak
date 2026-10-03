// Dev-only fixtures so the UI can be previewed in a plain browser (`npm run dev`)
// where there is no Rust backend. Never imported in production builds.

import type { ResultDto, SecondaryDto } from "./ipc";
import type { SettingsDto } from "./settings-ipc";

const appActions: SecondaryDto[] = [
  { label: "Show in folder", modifier: "ctrl", kind: "reveal_path" },
  { label: "Copy path", modifier: "shift", kind: "copy_text" },
  { label: "Run as administrator", modifier: "alt", kind: "run_as_admin" },
];
const fileActions: SecondaryDto[] = appActions.slice(0, 2);
const noExtras = { secondary: [], copy_text: null };

const rows: ResultDto[] = [
  { id: "m:chrome", title: "Google Chrome", subtitle: "Application", icon: { kind: "builtin", name: "app" }, plugin_id: "apps", action: "launch", secondary: appActions, copy_text: "C:/ProgramData/Start Menu/Google Chrome.lnk" },
  { id: "m:calc", title: "Calculator", subtitle: "Application", icon: { kind: "builtin", name: "calculator" }, plugin_id: "apps", action: "launch", ...noExtras },
  { id: "m:file", title: "quarterly-report-final-v2.xlsx", subtitle: "C:/Users/someone/Documents/Reports/2026/Q3/quarterly-report-final-v2.xlsx", icon: { kind: "builtin", name: "file" }, plugin_id: "files", action: "open_path", secondary: fileActions, copy_text: "C:/Users/someone/Documents/Reports/2026/Q3/quarterly-report-final-v2.xlsx" },
  { id: "m:folder", title: "Projects", subtitle: "D:/Projects", icon: { kind: "builtin", name: "folder" }, plugin_id: "files", action: "open_path", secondary: fileActions, copy_text: "D:/Projects", autocomplete: "D:/Projects/" },
  { id: "m:web", title: "Search Google for “rust traits”", subtitle: "Web search", icon: { kind: "builtin", name: "web" }, plugin_id: "web:g", action: "open_url", secondary: [{ label: "Copy URL", modifier: "shift", kind: "copy_text" }], copy_text: "https://www.google.com/search?q=rust%20traits" },
  { id: "m:copy", title: "8", subtitle: "2+2*3 · Enter to copy", icon: { kind: "builtin", name: "copy" }, plugin_id: "calculator", action: "copy_text", secondary: [], copy_text: "8" },
  { id: "m:broken", title: "Broken icon app", subtitle: "Falls back to the app glyph", icon: { kind: "url", url: "http://sevak-icon.localhost/0000000000000000" }, plugin_id: "apps", action: "launch", ...noExtras },
  { id: "m:shell", title: "Run `git status` in terminal", subtitle: "Runs only when you press Enter", icon: { kind: "builtin", name: "terminal" }, plugin_id: "shell", action: "custom", ...noExtras },
  { id: "m:plugin", title: "Custom plugin result", subtitle: "Plugin", icon: { kind: "builtin", name: "plugin" }, plugin_id: "x", action: "custom", ...noExtras },
  { id: "m:9", title: "Ninth row", subtitle: "Scrolls the list", icon: null, plugin_id: "apps", action: "launch", ...noExtras },
  { id: "m:10", title: "Tenth row", subtitle: "Scrolls the list", icon: null, plugin_id: "apps", action: "launch", ...noExtras },
];

export function mockHistory(): string[] {
  return ["g rust traits", "chrome", "~/Documents/"];
}

export function mockSearch(query: string): ResultDto[] {
  return query.trim() === "none" ? [] : rows;
}

export function mockSettings(): SettingsDto {
  return {
    config: {
      general: { hotkey: "Alt+Space", hide_on_blur: true, launch_at_login: false, check_for_updates: true },
      window: { width: 720 },
      linux: { wayland_use_xwayland: true },
      search: { max_results: 8, fallback_web_search: "g", query_history: true },
      appearance: { theme: "system" },
      plugins: { disabled: ["uuid"] },
      files: {
        directories: ["~/Desktop", "~/Documents", "~/Downloads"],
        max_depth: 4,
        include_hidden: false,
        keyword: "f",
        global: true,
      },
      shell: { terminal: "", shell: "", keep_open: true },
      web_search: [
        { keyword: "g", name: "Google", url: "https://www.google.com/search?q={query}" },
        { keyword: "yt", name: "YouTube", url: "https://www.youtube.com/results?search_query={query}" },
        { keyword: "gh", name: "GitHub", url: "https://github.com/search?q={query}" },
      ],
    },
    catalog: [
      { id: "apps", name: "Applications", description: "Launches installed applications.", keyword: null, enabled: true },
      { id: "calculator", name: "Calculator", description: "Evaluates math expressions as you type; Enter copies the result.", keyword: null, enabled: true },
      { id: "files", name: "Files", description: "Finds files and folders in your configured directories.", keyword: "f", enabled: true },
      { id: "shell", name: "Terminal commands", description: "Type `> command` to run it in a terminal; recent commands are offered again.", keyword: ">", enabled: true },
      { id: "uuid", name: "UUID generator", description: "Type `uuid ` to generate random UUIDs; Enter copies one.", keyword: "uuid", enabled: false },
    ],
    display: new URLSearchParams(location.search).get("display") === "wayland" ? "wayland" : "windows",
    is_gnome: true,
    config_path: "C:/Users/someone/AppData/Roaming/sevak/config.toml",
    log_dir: "C:/Users/someone/AppData/Roaming/sevak/logs",
    platform: "windows",
  };
}
