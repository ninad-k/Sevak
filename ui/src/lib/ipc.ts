// Typed wrappers around the Tauri IPC contract with the Rust shell.
// Every wrapper tolerates running in a plain browser (no Tauri runtime):
// failures are logged with console.warn instead of thrown.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type HotkeyMode = "global" | "external";

/** How a shortcut reaches Sevak. */
export type HotkeyMechanism =
  | "registered"
  | "windows_hook"
  | "spotlight_disabled"
  | "gnome_input_source_moved"
  | "fallback_key"
  | "desktop"
  | "inactive";

export interface HotkeyStatus {
  accelerator: string;
  mode: HotkeyMode;
  error: string | null;
  mechanism: HotkeyMechanism;
  /** One sentence on the mechanism (taken over with the keyboard hook, Spotlight's shortcut off...). */
  note: string | null;
  /** An OS shortcut stands in the way and the user may let Sevak take it (macOS, GNOME). */
  can_take_over: boolean;
  /** Sevak changed an OS shortcut with permission and can put it back. */
  can_restore: boolean;
}

export type ThemeSetting = "system" | "light" | "dark";

/** How one `[[hotkey]]` entry of the config fared, in config order. */
export interface CustomHotkeyStatus {
  key: string;
  description: string;
  /** Why it is not active, if it is not. */
  error: string | null;
  mechanism: HotkeyMechanism;
}

/** The validated appearance settings as CSS (see `appearance.ts`). */
export interface AppearanceCss {
  css: string;
  custom_css: string;
  /** Why a setting was ignored. */
  warnings: string[];
}

export interface Status {
  version: string;
  display: "windows" | "macos" | "x11" | "wayland" | "unknown";
  hotkey: HotkeyStatus;
  custom_hotkeys: CustomHotkeyStatus[];
  /** The Universal Actions shortcut; `null` when it is switched off. */
  actions_hotkey: CustomHotkeyStatus | null;
  appearance: AppearanceCss;
  /** The configured theme; `system` follows `prefers-color-scheme`. */
  theme: ThemeSetting;
  /** The search index is being (re)built. */
  indexing: boolean;
  /** Snippet expansion as you type: running, or why not. */
  snippet_expansion: { active: boolean; problem: string | null };
}

export type ActionKind =
  | "launch"
  | "open_path"
  | "open_url"
  | "copy_text"
  | "reveal_path"
  | "run_as_admin"
  | "paste_text"
  | "custom";

export type IconDto = { kind: "url"; url: string } | { kind: "builtin"; name: string };

/** A key held with Enter to run a secondary action. `ctrl` is Cmd on macOS. */
export type Modifier = "ctrl" | "shift" | "alt";

export interface SecondaryDto {
  label: string;
  modifier: Modifier | null;
  kind: ActionKind;
}

export interface ResultDto {
  id: string;
  title: string;
  subtitle: string;
  icon: IconDto | null;
  plugin_id: string;
  /** What Tab turns the input into, when the plugin offers a completion. */
  autocomplete?: string | null;
  action: ActionKind;
  /** Other actions; `execute`'s `action` index refers to this list. */
  secondary: SecondaryDto[];
  /** What Ctrl+C copies for this row, if anything. */
  copy_text: string | null;
  /** The row is a tile of the Grid View (shown as one when every row is). */
  tile?: boolean;
  /** The tile's picture when it is text (an emoji) rather than the icon. */
  glyph?: string | null;
  /** The row has a text to open in the Text View (Ctrl+T). */
  text_view?: boolean;
  /** Enter opens the Text View instead of running the action. */
  text_on_enter?: boolean;
  /** What Ctrl+L shows as Large Type instead of the title (a phone number, say). */
  large_text?: string | null;
}

export interface MetaRow {
  label: string;
  value: string;
}

/** The main content of the preview pane. */
export type PreviewBody =
  | { kind: "none" }
  | { kind: "text"; text: string; truncated: boolean }
  | { kind: "image"; src: string }
  | { kind: "folder"; entries: { name: string; dir: boolean }[]; truncated: boolean }
  | { kind: "url"; url: string };

/** What the preview pane draws for one result. */
export interface PreviewContent {
  title: string;
  subtitle: string;
  body: PreviewBody;
  meta: MetaRow[];
  /** Seconds since the unix epoch. */
  modified: number | null;
  /** Why there is no (or only a partial) preview. */
  note: string | null;
}

/** The full text of a result, for the Text View. */
export interface TextViewContent {
  title: string;
  text: string;
  truncated: boolean;
}

export type IndexState = "indexing" | "ready";

export interface IndexEvent {
  state: IndexState;
}

export const EVENT_SHOW = "sevak:show";
export const EVENT_HIDDEN = "sevak:hidden";
export const EVENT_STATUS = "sevak:status";
export const EVENT_INDEX = "sevak:index";
export const EVENT_RESULTS = "sevak:results";
export const EVENT_BUFFER_PROGRESS = "sevak:buffer-progress";

/** Hide the launcher window. */
export async function hideWindow(): Promise<void> {
  try {
    await invoke("hide_window");
  } catch (err) {
    console.warn("[ipc] hide_window failed:", err);
  }
}

/** Fetch the current shell status, or `null` when unavailable. */
export async function getStatus(): Promise<Status | null> {
  try {
    return await invoke<Status>("get_status");
  } catch (err) {
    console.warn("[ipc] get_status failed:", err);
    return null;
  }
}

/** Tell Rust the logical pixel height of the rendered content. */
export async function setContentHeight(height: number): Promise<void> {
  try {
    await invoke("set_content_height", { height });
  } catch (err) {
    console.warn("[ipc] set_content_height failed:", err);
  }
}

// True inside the Tauri webview; plain-browser `npm run dev` previews use mock data.
export function hasTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** One search's results; `ticket` names this set when executing one of them. */
export interface SearchResponse {
  ticket: number;
  results: ResultDto[];
}

/** Query the engine. Returns `null` on failure so callers can tell it from "no results". */
export async function search(query: string): Promise<SearchResponse | null> {
  if (import.meta.env.DEV && !hasTauri()) {
    const { mockSearch } = await import("./mock");
    return { ticket: 0, results: mockSearch(query) };
  }
  try {
    return await invoke<SearchResponse>("search", { query });
  } catch (err) {
    console.warn("[ipc] search failed:", err);
    return null;
  }
}

/**
 * What the preview pane shows for result `id` of search `ticket`. The shell
 * reads only what that result refers to; `null` when it could not.
 */
export async function preview(id: string, ticket: number): Promise<PreviewContent | null> {
  if (import.meta.env.DEV && !hasTauri()) {
    const { mockPreview } = await import("./mock");
    return mockPreview(id);
  }
  try {
    return await invoke<PreviewContent>("preview", { id, ticket });
  } catch (err) {
    console.warn("[ipc] preview failed:", err);
    return null;
  }
}

/** The full text of result `id` for the Text View, or `null`. */
export async function textView(id: string, ticket: number): Promise<TextViewContent | null> {
  if (import.meta.env.DEV && !hasTauri()) {
    const { mockTextView } = await import("./mock");
    return mockTextView(id);
  }
  try {
    return await invoke<TextViewContent>("text_view", { id, ticket });
  } catch (err) {
    console.warn("[ipc] text_view failed:", err);
    return null;
  }
}

/** Executed queries, most recent first (empty when history is off). */
export async function queryHistory(): Promise<string[]> {
  if (import.meta.env.DEV && !hasTauri()) {
    const { mockHistory } = await import("./mock");
    return mockHistory();
  }
  try {
    return await invoke<string[]>("query_history");
  } catch (err) {
    console.warn("[ipc] query_history failed:", err);
    return [];
  }
}

/**
 * Run result `id` of search `ticket` (the results on screen). `action` is the
 * index of one of its secondary actions; omit it for the primary action.
 * Resolves to an error message, or `null` on success.
 */
export async function execute(
  id: string,
  ticket: number,
  action?: number,
): Promise<string | null> {
  if (import.meta.env.DEV && !hasTauri()) {
    return id === "m:broken" ? "Could not start “Broken icon app” (preview error)" : null;
  }
  try {
    await invoke("execute", { id, ticket, action: action ?? null });
    return null;
  } catch (err) {
    console.warn("[ipc] execute failed:", err);
    return errorText(err);
  }
}

/** Copy result `id`'s most useful text (see `ResultDto.copy_text`) and hide. */
export async function copyResult(id: string, ticket: number): Promise<string | null> {
  if (import.meta.env.DEV && !hasTauri()) return null;
  try {
    await invoke("copy_result", { id, ticket });
    return null;
  } catch (err) {
    console.warn("[ipc] copy_result failed:", err);
    return errorText(err);
  }
}

/**
 * Stretch the window over the screen (`true`) for Large Type, or restore it.
 * Resolves to whether the window now covers the screen; `false` means the
 * caller should show the text inside the launcher instead.
 */
export async function setLargeType(on: boolean): Promise<boolean> {
  if (!hasTauri()) return on;
  try {
    await invoke("set_large_type", { on });
    return on;
  } catch (err) {
    console.warn("[ipc] set_large_type failed:", err);
    return false;
  }
}

function errorText(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

async function safeListen<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  try {
    return await listen<T>(event, (e) => cb(e.payload));
  } catch (err) {
    console.warn(`[ipc] listen(${event}) failed:`, err);
    return () => {};
  }
}

/** An action the launcher window carries out itself instead of the shell. */
export type WindowAction =
  | { kind: "large_type"; text: string }
  | { kind: "search"; query: string };

/** One Universal Actions entry: a result row, and what the window does for it, if anything. */
export interface SelectionActionDto extends ResultDto {
  window: WindowAction | null;
}

/** The actions for what was selected in another app (Universal Actions). */
export interface SelectionPayload {
  /** Names the set when executing one of the actions. */
  ticket: number;
  /** "Selected text", "2 selected files and folders", ... */
  title: string;
  /** A short preview of the selection. */
  subtitle: string;
  actions: SelectionActionDto[];
}

/** Text a workflow's Large Type or text view node shows. */
export interface OutputPayload {
  kind: "large_type" | "text_view";
  heading: string;
  text: string;
}

/** What the launcher is asked to show with (`sevak --query`, hotkey entries, errors). */
export interface ShowPayload {
  /** Text to put in the search field. */
  query: string | null;
  /** A message to show instead of results. */
  error: string | null;
  /** Actions for the selection captured by Universal Actions. */
  selection?: SelectionPayload | null;
  /** Output of a workflow (Large Type or a block of text). */
  output?: OutputPayload | null;
}

/** Window is being shown: clear the query, focus the input, apply the payload. */
export function onShow(cb: (payload: ShowPayload | null) => void): Promise<UnlistenFn> {
  return safeListen<ShowPayload | null>(EVENT_SHOW, cb);
}

/**
 * Asks for a show request that arrived before this page was listening. Also
 * tells Rust the page is ready to hear later ones directly.
 */
export async function takePendingShow(): Promise<ShowPayload | null> {
  try {
    return await invoke<ShowPayload | null>("take_pending_show");
  } catch (err) {
    console.warn("[ipc] take_pending_show failed:", err);
    return null;
  }
}

/** Window was hidden: clear the query. */
export function onHidden(cb: () => void): Promise<UnlistenFn> {
  return safeListen<void>(EVENT_HIDDEN, () => cb());
}

/** Config reloaded / hotkey status changed. */
export function onStatus(cb: (status: Status) => void): Promise<UnlistenFn> {
  return safeListen<Status>(EVENT_STATUS, cb);
}

/**
 * A slow plugin (a script plugin) has answers for the query on screen: run the
 * query again to pick them up.
 */
export function onResultsUpdated(cb: () => void): Promise<UnlistenFn> {
  return safeListen<string>(EVENT_RESULTS, () => cb());
}

/** The search index started or finished (re)building. */
export function onIndex(cb: (state: IndexState) => void): Promise<UnlistenFn> {
  return safeListen<IndexEvent>(EVENT_INDEX, (e) => cb(e.state));
}

// ---- File buffer: files collected from the results to act on together ----

/** One collected file or folder. */
export interface BufferItemDto {
  path: string;
  name: string;
  is_dir: boolean;
  icon: IconDto | null;
}

/** An action offered for the whole buffer. */
export interface BufferActionDto {
  key: string;
  label: string;
  /** Asks for a destination folder next. */
  destination: boolean;
}

export interface BufferDto {
  items: BufferItemDto[];
  actions: BufferActionDto[];
}

/** How a buffer action ended. */
export interface BufferRunDto {
  message: string;
  /** Everything worked. */
  ok: boolean;
  /** The user cancelled the confirmation. */
  declined: boolean;
  /** The launcher went away (the action handed over, or copied). */
  hidden: boolean;
  buffer: BufferDto;
}

/** A slow buffer action's progress. */
export interface BufferProgress {
  label: string;
  done: number;
  total: number;
  name: string;
}

/** What the buffer says about the last action. */
export interface BufferNote {
  text: string;
  error: boolean;
}

/** Where Move to… / Copy to… puts the items. */
export type BufferDestination =
  | { kind: "result"; id: string; ticket: number }
  | { kind: "text"; text: string };

const inPreview = () => import.meta.env.DEV && !hasTauri();

/** The buffer as it is now. */
export async function fileBufferGet(): Promise<BufferDto | null> {
  if (inPreview()) return (await import("./mock")).mockBuffer.get();
  try {
    return await invoke<BufferDto>("file_buffer_get");
  } catch (err) {
    console.warn("[ipc] file_buffer_get failed:", err);
    return null;
  }
}

/** Adds result `id` of search `ticket`. Resolves to the buffer, or an error message. */
export async function fileBufferAdd(id: string, ticket: number): Promise<BufferDto | string> {
  if (inPreview()) return (await import("./mock")).mockBuffer.add(id);
  try {
    return await invoke<BufferDto>("file_buffer_add", { id, ticket });
  } catch (err) {
    console.warn("[ipc] file_buffer_add failed:", err);
    return errorText(err);
  }
}

/** Removes the item at `index`, or the last one. */
export async function fileBufferRemove(index?: number): Promise<BufferDto | null> {
  if (inPreview()) return (await import("./mock")).mockBuffer.remove(index);
  try {
    return await invoke<BufferDto>("file_buffer_remove", { index: index ?? null });
  } catch (err) {
    console.warn("[ipc] file_buffer_remove failed:", err);
    return null;
  }
}

export async function fileBufferClear(): Promise<BufferDto | null> {
  if (inPreview()) return (await import("./mock")).mockBuffer.clear();
  try {
    return await invoke<BufferDto>("file_buffer_clear");
  } catch (err) {
    console.warn("[ipc] file_buffer_clear failed:", err);
    return null;
  }
}

/** Runs buffer action `key` on everything collected. Resolves to how it went, or an error message. */
export async function fileBufferRun(
  key: string,
  destination: BufferDestination | null,
): Promise<BufferRunDto | string> {
  if (inPreview()) return (await import("./mock")).mockBuffer.run(key, destination);
  try {
    return await invoke<BufferRunDto>("file_buffer_run", { key, destination });
  } catch (err) {
    console.warn("[ipc] file_buffer_run failed:", err);
    return errorText(err);
  }
}

/** The Universal Actions for the collected files ("More file actions…"). */
export async function fileBufferSelection(): Promise<SelectionPayload | string> {
  if (inPreview()) return (await import("./mock")).mockSelection();
  try {
    return await invoke<SelectionPayload>("file_buffer_selection");
  } catch (err) {
    console.warn("[ipc] file_buffer_selection failed:", err);
    return errorText(err);
  }
}

/** Progress of a slow buffer action (move, copy, trash, zip). */
export async function onBufferProgress(
  cb: (progress: BufferProgress) => void,
): Promise<UnlistenFn> {
  if (inPreview()) return (await import("./mock")).mockBuffer.onProgress(cb);
  return safeListen<BufferProgress>(EVENT_BUFFER_PROGRESS, cb);
}
