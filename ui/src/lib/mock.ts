// Dev-only fixtures so the UI can be previewed in a plain browser (`npm run dev`)
// where there is no Rust backend. Never imported in production builds.

import type {
  BufferActionDto,
  BufferDestination,
  BufferDto,
  BufferItemDto,
  BufferProgress,
  BufferRunDto,
  PreviewContent,
  ResultDto,
  SecondaryDto,
  SelectionActionDto,
  SelectionPayload,
  TextViewContent,
} from "./ipc";
import type { SettingsDto } from "./settings-ipc";

const appActions: SecondaryDto[] = [
  { label: "Show in folder", modifier: "ctrl", kind: "reveal_path" },
  { label: "Copy path", modifier: "shift", kind: "copy_text" },
  { label: "Run as administrator", modifier: "alt", kind: "run_as_admin" },
];
const fileActions: SecondaryDto[] = appActions.slice(0, 2);
const noExtras = { secondary: [], copy_text: null };

const longText = [
  "Dear team,",
  "",
  "Thanks for the quick turnaround on the quarterly figures. A few notes before the review on Thursday:",
  "",
  "  1. Revenue is up 8.4% quarter over quarter, driven mostly by the new annual plans.",
  "  2. Support volume dropped for the third month in a row; the new onboarding guide seems to be working.",
  "  3. The infrastructure bill came in 6% under forecast after the storage clean-up.",
  "",
  "Please send corrections by Wednesday noon so they can go into the deck. If anything looks off, reply here",
  "rather than editing the spreadsheet directly, because several people are building on it.",
  "",
  "Open questions for Thursday:",
  "  - Do we keep the discount for annual renewals next quarter?",
  "  - Who owns the migration of the legacy reports?",
  "  - Is the new region launch still on for November?",
  "",
  "Best regards,",
  "Ninad",
].join("\n");

/** A small picture for the image preview (an SVG, so the fixture stays text). */
const sunset = `data:image/svg+xml;utf8,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1e1b4b"/><stop offset=".55" stop-color="#f97316"/><stop offset="1" stop-color="#fde68a"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><circle cx="320" cy="230" r="64" fill="#fff7ed"/><rect y="270" width="640" height="90" fill="#0f172a"/></svg>',
)}`;

/** The emoji of the grid preview: `[glyph, name, keywords]`. */
const emoji: [string, string, string][] = [
  ["😀", "grinning face", "happy smile"],
  ["😂", "face with tears of joy", "lol laugh"],
  ["🙂", "slightly smiling face", "smile"],
  ["😉", "winking face", "wink"],
  ["😍", "smiling face with heart-eyes", "love"],
  ["😎", "smiling face with sunglasses", "cool"],
  ["🤔", "thinking face", "hmm"],
  ["😴", "sleeping face", "zzz"],
  ["😭", "loudly crying face", "sad"],
  ["😡", "enraged face", "angry"],
  ["👍", "thumbs up", "yes like"],
  ["👎", "thumbs down", "no dislike"],
  ["👏", "clapping hands", "applause"],
  ["🙏", "folded hands", "thanks please"],
  ["💪", "flexed biceps", "strong"],
  ["👀", "eyes", "look"],
  ["❤️", "red heart", "love"],
  ["💔", "broken heart", "sad"],
  ["🔥", "fire", "hot lit"],
  ["✨", "sparkles", "shiny"],
  ["🎉", "party popper", "celebrate"],
  ["💯", "hundred points", "perfect"],
  ["✅", "check mark button", "done yes"],
  ["❌", "cross mark", "no wrong"],
  ["⚠️", "warning", "caution"],
  ["🚀", "rocket", "launch ship"],
  ["🐛", "bug", "insect"],
  ["💡", "light bulb", "idea"],
  ["📌", "pushpin", "pin"],
  ["📎", "paperclip", "attach"],
  ["📅", "calendar", "date"],
  ["☕", "hot beverage", "coffee"],
  ["🍕", "pizza", "food"],
  ["🌈", "rainbow", "weather"],
  ["⭐", "star", "favorite"],
  ["🎯", "bullseye", "target"],
];

function emojiRows(words: string): ResultDto[] {
  const terms = words.toLowerCase().split(/\s+/).filter(Boolean);
  return emoji
    .filter(([, name, keywords]) => terms.every((t) => `${name} ${keywords}`.includes(t)))
    .map(([glyph, name]) => ({
      id: `emoji:word:${glyph}`,
      title: name,
      subtitle: `${glyph} · Enter to paste`,
      icon: null,
      plugin_id: "emoji:word",
      action: "paste_text" as const,
      secondary: [{ label: "Copy emoji", modifier: "shift" as const, kind: "copy_text" as const }],
      copy_text: glyph,
      tile: true,
      glyph,
    }));
}

const sampleCode = `//! Sevak's launcher window.
use tauri::{AppHandle, Manager};

pub fn toggle(app: &AppHandle) {
    let visible = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if visible {
        hide(app);
    } else {
        show(app);
    }
}
`;

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
  { id: "m:code", title: "window.rs", subtitle: "D:/Projects/sevak/src-tauri/src/window.rs", icon: { kind: "builtin", name: "file" }, plugin_id: "files", action: "open_path", secondary: fileActions, copy_text: "D:/Projects/sevak/src-tauri/src/window.rs" },
  { id: "m:image", title: "sunset.svg", subtitle: "C:/Users/someone/Pictures/sunset.svg", icon: { kind: "builtin", name: "file" }, plugin_id: "files", action: "open_path", secondary: fileActions, copy_text: "C:/Users/someone/Pictures/sunset.svg" },
  { id: "m:long", title: "Dear team, thanks for the quick turnaround on the quarterly figures and…", subtitle: "Copied 12 minutes ago · Enter to paste", icon: { kind: "builtin", name: "copy" }, plugin_id: "clipboard", action: "paste_text", secondary: [], copy_text: longText, text_view: true },
  { id: "m:output", title: "Disk usage report", subtitle: "Enter to read the output", icon: { kind: "builtin", name: "terminal" }, plugin_id: "script:disk", action: "copy_text", secondary: [], copy_text: longText, text_view: true, text_on_enter: true },
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
  const text = query.trim();
  if (text === "none") return [];
  // The emoji picker: `:smile` or `emoji smile` shows a grid.
  const grid = /^(?::|emoji\s)\s*(.*)$/.exec(text);
  if (grid) return emojiRows(grid[1]);
  return rows;
}

/** Previews of the fixtures above, shaped like the shell's `preview` answer. */
export function mockPreview(id: string): PreviewContent | null {
  const base = { modified: 1_790_000_000, note: null };
  const row = rows.find((r) => r.id === id) ?? mockSearch(":").find((r) => r.id === id);
  if (!row) return null;
  const common = { title: row.title, subtitle: row.subtitle, ...base };
  switch (id) {
    case "m:file":
      return { ...common, body: { kind: "none" }, note: "No preview for this kind of file", meta: [
        { label: "Kind", value: "Excel workbook" },
        { label: "Size", value: "184.3 KB" },
        { label: "Path", value: row.copy_text ?? "" },
      ] };
    case "m:image":
      return { ...common, body: { kind: "image", src: sunset }, meta: [
        { label: "Kind", value: "SVG image" },
        { label: "Size", value: "612 bytes" },
        { label: "Path", value: row.copy_text ?? "" },
      ] };
    case "m:folder":
      return { ...common, body: { kind: "folder", truncated: false, entries: [
        { name: "sevak", dir: true },
        { name: "website", dir: true },
        { name: "notes", dir: true },
        { name: "archive", dir: true },
        { name: "main.rs", dir: false },
        { name: "README.md", dir: false },
        { name: "todo.txt", dir: false },
        { name: "budget.xlsx", dir: false },
      ] }, meta: [
        { label: "Kind", value: "Folder" },
        { label: "Items", value: "8" },
        { label: "Path", value: row.copy_text ?? "" },
      ] };
    case "m:web":
      return { ...common, body: { kind: "url", url: row.copy_text ?? "" }, meta: [{ label: "Site", value: "www.google.com" }] };
    case "m:copy":
      return { ...common, body: { kind: "none" }, meta: [
        { label: "Result", value: "8" },
        { label: "Calculation", value: "2 + 2 * 3" },
      ] };
    case "m:long":
    case "m:output":
      return { ...common, body: { kind: "text", text: longText, truncated: false }, meta: [] };
    case "m:chrome":
    case "m:calc":
      return { ...common, body: { kind: "none" }, meta: [
        { label: "Kind", value: "Application" },
        { label: "Shortcut", value: "C:/ProgramData/Microsoft/Windows/Start Menu/Programs/" + row.title + ".lnk" },
      ] };
    case "m:code":
      return { ...common, body: { kind: "text", text: sampleCode, truncated: true }, meta: [] };
  }
  if (row.glyph) {
    const found = emoji.find(([glyph]) => glyph === row.glyph);
    return { ...common, body: { kind: "none" }, meta: [
      { label: "Name", value: row.title },
      { label: "Keywords", value: (found?.[2] ?? "").split(" ").join(", ") },
      { label: "Code points", value: [...row.glyph].map((c) => `U+${c.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")}`).join(" ") },
    ] };
  }
  return { ...common, body: { kind: "none" }, meta: [] };
}

export function mockTextView(id: string): TextViewContent | null {
  const row = rows.find((r) => r.id === id);
  if (!row?.text_view) return null;
  return { title: row.title, text: `${longText}\n\n${longText}`, truncated: false };
}

export function mockSettings(): SettingsDto {
  return {
    config: {
      general: {
        hotkey: "Super+Space",
        actions_hotkey: "Ctrl+Alt+Space",
        accept_injected_hotkeys: false,
        hide_on_blur: true,
        launch_at_login: false,
        check_for_updates: true,
        update_channel: "stable",
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
        blur: false,
        radius: 14,
        theme_file: "",
        custom_css: "",
      },
      plugins: { disabled: ["uuid"] },
      calculator: { currency: false },
      snippets: {
        auto_expand: false,
        prefix: "",
        expand_on: "immediate",
        case_sensitive: true,
        ignore_apps: [],
        expand_in_terminals: false,
        expand_in_browsers: false,
      },
      files: {
        directories: ["~/Desktop", "~/Documents", "~/Downloads"],
        max_depth: 4,
        include_hidden: false,
        keyword: "f",
        global: true,
        use_os_index: true,
        index_keyword: "ff",
        content_keyword: "in",
        allow_network_paths: false,
      },
      bookmarks: { browsers: [], keyword: "b", global: true },
      paste: { restore_clipboard: false },
      file_buffer: { keep_between_shows: false },
      clipboard: {
        enabled: false,
        max_items: 200,
        max_item_bytes: 64 * 1024,
        ignore_apps: [],
        default_ignore_apps: true,
        images: true,
        files: true,
        max_image_bytes: 10 * 1024 * 1024,
        encrypt: true,
      },
      contacts: { enabled: false, keyword: "c", use_system: true, vcard_files: [] },
      onepassword: { enabled: false, keyword: "1p", op_path: "", account: "", cache_minutes: 10 },
      dictionary: { define_keyword: "define", spell_keyword: "spell", use_system: true },
      ai: {
        enabled: false,
        keyword: "ai",
        provider: "ollama",
        model: "",
        base_url: "",
        system_prompt: "",
        max_tokens: 512,
        timeout_secs: 60,
      },
      system: { confirm: true, disabled: [] },
      tasks: { confirm: true, disabled: [], keyword: "t", global: true },
      media: { keyword: "play", global: true, now_playing: true },
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
    display: mockDisplay(),
    is_gnome: true,
    config_path: "C:/Users/someone/AppData/Roaming/sevak/config.toml",
    log_dir: "C:/Users/someone/AppData/Roaming/sevak/logs",
    platform: mockPlatform(),
  };
}

/** `?platform=macos|linux` previews the platform notes; Windows otherwise. */
function mockPlatform(): "windows" | "macos" | "linux" {
  const asked = new URLSearchParams(location.search).get("platform");
  return asked === "macos" || asked === "linux" ? asked : "windows";
}

/** `?display=wayland|x11` previews the Linux pages; the platform's own display otherwise. */
function mockDisplay(): SettingsDto["display"] {
  const asked = new URLSearchParams(location.search).get("display");
  if (asked === "wayland" || asked === "x11") return asked;
  const platform = mockPlatform();
  return platform === "macos" ? "macos" : platform === "linux" ? "x11" : "windows";
}
