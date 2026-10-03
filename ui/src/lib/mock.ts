// Dev-only fixtures so the UI can be previewed in a plain browser (`npm run dev`)
// where there is no Rust backend. Never imported in production builds.

import type {
  BufferActionDto,
  BufferDestination,
  BufferDto,
  BufferItemDto,
  BufferProgress,
  BufferRunDto,
  ResultDto,
  SecondaryDto,
  SelectionActionDto,
  SelectionPayload,
} from "./ipc";
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
  { id: "m:file2", title: "budget-2026.pdf", subtitle: "~/Documents/Finance", icon: { kind: "builtin", name: "file" }, plugin_id: "files", action: "open_path", secondary: fileActions, copy_text: "C:/Users/someone/Documents/Finance/budget-2026.pdf" },
  { id: "m:file3", title: "meeting-notes.md", subtitle: "~/Documents", icon: { kind: "builtin", name: "file" }, plugin_id: "files", action: "open_path", secondary: fileActions, copy_text: "C:/Users/someone/Documents/meeting-notes.md" },
  { id: "m:folder2", title: "Documents", subtitle: "~", icon: { kind: "builtin", name: "folder" }, plugin_id: "files", action: "open_path", secondary: fileActions, copy_text: "C:/Users/someone/Documents", autocomplete: "~/Documents/" },
  { id: "m:folder3", title: "Downloads", subtitle: "~", icon: { kind: "builtin", name: "folder" }, plugin_id: "files", action: "open_path", secondary: fileActions, copy_text: "C:/Users/someone/Downloads", autocomplete: "~/Downloads/" },
  { id: "m:web", title: "Search Google for “rust traits”", subtitle: "Web search", icon: { kind: "builtin", name: "web" }, plugin_id: "web:g", action: "open_url", secondary: [{ label: "Copy URL", modifier: "shift", kind: "copy_text" }], copy_text: "https://www.google.com/search?q=rust%20traits" },
  { id: "m:copy", title: "8", subtitle: "2+2*3 · Enter to copy", icon: { kind: "builtin", name: "copy" }, plugin_id: "calculator", action: "copy_text", secondary: [], copy_text: "8" },
  { id: "m:broken", title: "Broken icon app", subtitle: "Falls back to the app glyph", icon: { kind: "url", url: "http://sevak-icon.localhost/0000000000000000" }, plugin_id: "apps", action: "launch", ...noExtras },
  { id: "m:shell", title: "Run `git status` in terminal", subtitle: "Runs only when you press Enter", icon: { kind: "builtin", name: "terminal" }, plugin_id: "shell", action: "custom", ...noExtras },
  { id: "m:plugin", title: "Custom plugin result", subtitle: "Plugin", icon: { kind: "builtin", name: "plugin" }, plugin_id: "x", action: "custom", ...noExtras },
  { id: "m:9", title: "Ninth row", subtitle: "Scrolls the list", icon: null, plugin_id: "apps", action: "launch", ...noExtras },
  { id: "m:10", title: "Tenth row", subtitle: "Scrolls the list", icon: null, plugin_id: "apps", action: "launch", ...noExtras },
];

/** Universal Actions for a selected sentence, shown by the preview at `/#selection`. */
export function mockSelection(): SelectionPayload {
  const copyIt: SecondaryDto = { label: "Copy", modifier: "ctrl", kind: "copy_text" };
  const row = (
    key: string,
    title: string,
    subtitle: string,
    action: SelectionActionDto["action"],
    extras: Partial<SelectionActionDto> = {},
  ): SelectionActionDto => ({
    id: `selection:${key}`,
    title,
    subtitle,
    icon: null,
    plugin_id: "selection",
    action,
    secondary: [],
    copy_text: null,
    window: null,
    ...extras,
  });
  return {
    ticket: 0,
    title: "Selected text",
    subtitle: "the quick brown fox jumps over the lazy dog",
    actions: [
      row("search:g", "Search Google for “the quick brown fox…”", "https://www.google.com/search?q=the%20quick%20brown%20fox", "open_url", {
        secondary: [{ label: "Copy URL", modifier: "shift", kind: "copy_text" }],
      }),
      row("search:yt", "Search YouTube for “the quick brown fox…”", "https://www.youtube.com/results?search_query=the%20quick", "open_url"),
      row("large_type", "Show as Large Type", "", "custom", {
        window: { kind: "large_type", text: "the quick brown fox jumps over the lazy dog" },
      }),
      row("copy", "Copy text", "the quick brown fox jumps over the lazy dog", "copy_text"),
      row("paste_plain", "Paste as plain text", "Replaces the selection without its formatting", "paste_text"),
      row("t:upper", "Uppercase", "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG", "paste_text", { secondary: [copyIt] }),
      row("t:title", "Title Case", "The Quick Brown Fox Jumps Over The Lazy Dog", "paste_text", { secondary: [copyIt] }),
      row("t:base64_encode", "Base64 encode", "dGhlIHF1aWNrIGJyb3duIGZveCBqdW1wcyBvdmVyIHRoZSBsYXp5IGRvZw==", "paste_text", { secondary: [copyIt] }),
    ],
  };
}

/** The files-plugin rows of the preview, as buffer items. */
function bufferItemFor(id: string): BufferItemDto | null {
  const row = rows.find((r) => r.id === id && r.plugin_id === "files");
  if (!row?.copy_text) return null;
  return {
    path: row.copy_text,
    name: row.title,
    is_dir: !!row.autocomplete,
    icon: row.icon,
  };
}

const bufferActions: BufferActionDto[] = [
  { key: "open_all", label: "Open all", destination: false },
  { key: "show_in_folder", label: "Show in folder", destination: false },
  { key: "copy_paths", label: "Copy paths", destination: false },
  { key: "copy_files", label: "Copy files to clipboard", destination: false },
  { key: "move_to", label: "Move to…", destination: true },
  { key: "copy_to", label: "Copy to…", destination: true },
  { key: "trash", label: "Move to Trash", destination: false },
  { key: "zip", label: "Compress to .zip", destination: false },
  { key: "open_terminal", label: "Open in terminal", destination: false },
];

/** In-memory file buffer for the browser preview (the real one lives in the shell). */
let previewItems: BufferItemDto[] = [];
const progressListeners = new Set<(progress: BufferProgress) => void>();
const snapshot = (): BufferDto => ({ items: [...previewItems], actions: bufferActions });
const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export const mockBuffer = {
  get: (): BufferDto => snapshot(),
  add(id: string): BufferDto | string {
    const item = bufferItemFor(id);
    if (!item) return "Only files and folders from the file results can be collected.";
    if (!previewItems.some((i) => i.path === item.path)) previewItems.push(item);
    return snapshot();
  },
  remove(index?: number): BufferDto {
    if (index === undefined) previewItems.pop();
    else previewItems = previewItems.filter((_, i) => i !== index);
    return snapshot();
  },
  clear(): BufferDto {
    previewItems = [];
    return snapshot();
  },
  /** A few seeded items, for `/#buffer`. */
  seed(): BufferDto {
    previewItems = ["m:file", "m:folder", "m:file2", "m:file3"].flatMap((id) => {
      const item = bufferItemFor(id);
      return item ? [item] : [];
    });
    return snapshot();
  },
  /** Pretends to run an action, with progress, so the progress row can be seen. */
  async run(key: string, destination: BufferDestination | null): Promise<BufferRunDto | string> {
    const action = bufferActions.find((a) => a.key === key);
    if (!action) return "that action is not available";
    if (action.destination && !destination) return "Pick a destination folder first.";
    const total = previewItems.length;
    if (["move_to", "copy_to", "trash", "zip"].includes(key)) {
      for (let done = 0; done <= total; done++) {
        for (const listener of progressListeners) {
          listener({ label: action.label, done, total, name: previewItems[done]?.name ?? "" });
        }
        await pause(450);
      }
    }
    const consumed = key === "move_to" || key === "trash";
    const message = `${action.label}: ${total} items (preview, nothing was changed).`;
    if (consumed) previewItems = [];
    return { message, ok: true, declined: false, hidden: false, buffer: snapshot() };
  },
  onProgress(cb: (progress: BufferProgress) => void) {
    progressListeners.add(cb);
    return () => void progressListeners.delete(cb);
  },
};

export function mockHistory(): string[] {
  return ["g rust traits", "chrome", "~/Documents/"];
}

export function mockSearch(query: string): ResultDto[] {
  return query.trim() === "none" ? [] : rows;
}

export function mockSettings(): SettingsDto {
  return {
    config: {
      general: {
        hotkey: "Alt+Space",
        actions_hotkey: "Ctrl+Alt+Space",
        hide_on_blur: true,
        launch_at_login: false,
        check_for_updates: true,
      },
      window: { width: 720 },
      linux: { wayland_use_xwayland: true },
      actions: { use_primary_selection: true, use_clipboard_fallback: false },
      search: { max_results: 8, fallback_web_search: "g", query_history: true },
      appearance: {
        theme: "system",
        accent: "",
        font_size: 15,
        font_family: "",
        opacity: 100,
        radius: 14,
        custom_css: "",
      },
      plugins: { disabled: ["uuid"] },
      calculator: { currency: false },
      files: {
        directories: ["~/Desktop", "~/Documents", "~/Downloads"],
        max_depth: 4,
        include_hidden: false,
        keyword: "f",
        global: true,
      },
      bookmarks: { browsers: [], keyword: "b", global: true },
      shell: { terminal: "", shell: "", keep_open: true },
      web_search: [
        { keyword: "g", name: "Google", url: "https://www.google.com/search?q={query}" },
        { keyword: "yt", name: "YouTube", url: "https://www.youtube.com/results?search_query={query}" },
        { keyword: "gh", name: "GitHub", url: "https://github.com/search?q={query}" },
      ],
      hotkey: [
        { key: "Ctrl+Alt+T", query: "> " },
        { key: "Ctrl+Alt+F", run: "apps:firefox.desktop" },
      ],
    },
    catalog: [
      { id: "apps", name: "Applications", description: "Launches installed applications.", keyword: null, enabled: true },
      { id: "calculator", name: "Calculator", description: "Evaluates math expressions and converts units (and currencies, if enabled) as you type; Enter copies the result.", keyword: null, enabled: true },
      { id: "files", name: "Files", description: "Finds files and folders in your configured directories.", keyword: "f", enabled: true },
      { id: "bookmarks", name: "Bookmarks", description: "Finds bookmarks in your browsers (read from disk; nothing is sent anywhere).", keyword: "b", enabled: true },
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
