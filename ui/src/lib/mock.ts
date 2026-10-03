// Dev-only fixtures so the UI can be previewed in a plain browser (`npm run dev`)
// where there is no Rust backend. Never imported in production builds.

import type { ResultDto } from "./ipc";

const rows: ResultDto[] = [
  { id: "m:chrome", title: "Google Chrome", subtitle: "Application", icon: { kind: "builtin", name: "app" }, plugin_id: "apps", action: "launch" },
  { id: "m:calc", title: "Calculator", subtitle: "Application", icon: { kind: "builtin", name: "calculator" }, plugin_id: "apps", action: "launch" },
  { id: "m:file", title: "quarterly-report-final-v2.xlsx", subtitle: "C:/Users/someone/Documents/Reports/2026/Q3/quarterly-report-final-v2.xlsx", icon: { kind: "builtin", name: "file" }, plugin_id: "files", action: "open_path" },
  { id: "m:folder", title: "Projects", subtitle: "D:/Projects", icon: { kind: "builtin", name: "folder" }, plugin_id: "files", action: "open_path" },
  { id: "m:web", title: "Search Google for “rust traits”", subtitle: "Web search", icon: { kind: "builtin", name: "web" }, plugin_id: "web:g", action: "open_url" },
  { id: "m:copy", title: "8", subtitle: "2+2*3 · Enter to copy", icon: { kind: "builtin", name: "copy" }, plugin_id: "calculator", action: "copy_text" },
  { id: "m:broken", title: "Broken icon app", subtitle: "Falls back to the app glyph", icon: { kind: "url", url: "http://sevak-icon.localhost/0000000000000000" }, plugin_id: "apps", action: "launch" },
  { id: "m:plugin", title: "Custom plugin result", subtitle: "Plugin", icon: { kind: "builtin", name: "plugin" }, plugin_id: "x", action: "custom" },
  { id: "m:9", title: "Ninth row", subtitle: "Scrolls the list", icon: null, plugin_id: "apps", action: "launch" },
  { id: "m:10", title: "Tenth row", subtitle: "Scrolls the list", icon: null, plugin_id: "apps", action: "launch" },
];

export function mockSearch(query: string): ResultDto[] {
  return query.trim() === "none" ? [] : rows;
}
