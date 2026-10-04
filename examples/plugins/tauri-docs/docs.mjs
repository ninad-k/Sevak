// A small link index, not a copy of the documentation or a live site crawler.
export const pages = [
  ["Getting started", "start/", "introduction project overview"],
  ["Prerequisites", "start/prerequisites/", "install setup dependencies linux windows macos"],
  ["Create a project", "start/create-project/", "new scaffold vite svelte react"],
  ["Calling Rust from the frontend", "develop/calling-rust/", "invoke commands ipc backend"],
  ["Calling the frontend from Rust", "develop/calling-frontend/", "events emit listen ipc"],
  ["Application state", "develop/state-management/", "manage mutex shared rust"],
  ["Capabilities", "security/capabilities/", "security permissions access control"],
  ["Permissions", "security/permissions/", "allow deny scope security"],
  ["Configuration reference", "reference/config/", "tauri conf json build bundle windows"],
  ["JavaScript API", "reference/javascript/api/", "typescript package module"],
  ["Distribution", "distribute/", "build package installer release sign"],
  ["Updater", "plugin/updater/", "updates signature download release"],
  ["Global shortcut", "plugin/global-shortcut/", "keyboard hotkey register"],
  ["Clipboard", "plugin/clipboard/", "copy paste text image"],
  ["File system", "plugin/file-system/", "read write directory files fs"],
  ["Dialog", "plugin/dialog/", "open save message picker"],
  ["Shell", "plugin/shell/", "process command spawn sidecar"],
  ["Notification", "plugin/notification/", "notify alerts desktop"],
  ["Autostart", "plugin/autostart/", "login startup launch"],
  ["Window customization", "learn/window-customization/", "titlebar transparent decoration"],
  ["System tray", "learn/system-tray/", "icon menu background"],
];

export function results(query) {
  const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const matches = pages.map(([title,path,keywords]) => {
    const heading = title.toLowerCase();
    const haystack = `${heading} ${path} ${keywords}`;
    return { title, path, score: tokens.every(t => haystack.includes(t)) ? tokens.reduce((n,t) => n + (heading.includes(t) ? 3 : 1), 1) : 0 };
  }).filter(p => p.score > 0).sort((a,b) => b.score - a.score).slice(0, 12);
  if (!matches.length) return [{ key: "no-match", title: "No guide in the local index matches", subtitle: "Enter to browse the official Tauri 2 documentation", action: { type: "open_url", url: "https://v2.tauri.app/" } }];
  return matches.map(({title,path}) => ({ key: path, title, subtitle: `Tauri 2 · v2.tauri.app/${path}`, icon: { kind: "builtin", name: "web" }, action: { type: "open_url", url: `https://v2.tauri.app/${path}` } }));
}
