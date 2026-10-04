<script lang="ts">
  import { onMount, tick } from "svelte";
  import FileBufferStrip from "./lib/FileBufferStrip.svelte";
  import GridView from "./lib/GridView.svelte";
  import PreviewPane from "./lib/PreviewPane.svelte";
  import ResultRow from "./lib/ResultRow.svelte";
  import TextView from "./lib/TextView.svelte";
  import { displayAccelerator } from "./lib/accelerator";
  import { applyAppearance } from "./lib/appearance";
  import { parentPath } from "./lib/path";
  import { applyTheme } from "./lib/theme";
  import {
    copyResult,
    execute,
    fileBufferAdd,
    fileBufferClear,
    fileBufferGet,
    fileBufferRemove,
    fileBufferRun,
    fileBufferSelection,
    getStatus,
    hasTauri,
    hideWindow,
    onBufferProgress,
    onHidden,
    onIndex,
    onResultsUpdated,
    onShow,
    onStatus,
    preview,
    queryHistory,
    search,
    setContentHeight,
    setLargeType,
    takePendingShow,
    textView,
    type BufferDestination,
    type BufferDto,
    type BufferNote,
    type BufferProgress,
    type Modifier,
    type PreviewContent,
    type ResultDto,
    type OutputPayload,
    type SelectionActionDto,
    type SelectionPayload,
    type ShowPayload,
    type Status,
    type TextViewContent,
    type WindowAction,
  } from "./lib/ipc";

  let query = $state("");
  let results = $state<ResultDto[]>([]);
  /** The search that produced `results`, so Enter runs exactly what is shown. */
  let resultsTicket = 0;
  let selected = $state(0);
  let error = $state<string | null>(null);
  let indexing = $state(false);
  let status = $state<Status | null>(null);
  /** Icon URLs that failed to load; those rows show a built-in glyph instead. */
  let brokenIcons = $state<Record<string, boolean>>({});
  let input: HTMLInputElement | undefined = $state();
  let shell: HTMLElement | undefined = $state();
  let opening = $state(false);
  let list: HTMLElement | undefined = $state();

  /** The action panel (Right arrow / Ctrl+K): all actions of the selected row. */
  let panelOpen = $state(false);
  let panelIndex = $state(0);
  let panelEl: HTMLElement | undefined = $state();
  /**
   * Universal Actions: the actions for what was selected in another app. While
   * it is set the panel lists them instead of a row's actions (and is open).
   */
  let selection = $state<SelectionPayload | null>(null);
  /** A workflow's text view node: a block of text under the search bar. */
  let outputView = $state<{ heading: string; text: string } | null>(null);
  /** Large Type: the text shown huge, or `null` when it is not showing. */
  let largeText = $state<string | null>(null);
  /** The window was stretched over the screen (else the text shows inside the launcher). */
  let largeFull = $state(false);
  /** The window and font size are settled, so Large Type can be drawn. */
  let largeReady = $state(false);
  let largeFont = $state(96);
  /** Tags each open/close so a slow window resize cannot resurrect a dismissed Large Type. */
  let largeSeq = 0;

  /** The preview pane (Shift tap / Ctrl+Y) under the list; it follows the selection. */
  let previewOpen = $state(false);
  let previewContent = $state<PreviewContent | null>(null);
  let previewLoading = $state(false);
  let previewSeq = 0;
  /** When a lone Shift went down (0: none, or another key came between). */
  let shiftDownAt = 0;
  /** The Text View (Ctrl+T): the selected row's long text, full height. */
  let textViewOpen = $state(false);
  let textContent = $state<TextViewContent | null>(null);
  let textSeq = 0;
  let textViewEl: ReturnType<typeof TextView> | undefined = $state();
  /** Columns of the Grid View, measured by the grid. */
  let gridCols = $state(1);

  /**
   * The file buffer: files and folders collected from the file results
   * (Alt+Up / Alt+Down) to act on together. The shell owns the list; this is
   * the copy last reported.
   */
  let fileBuffer = $state<BufferDto>({ items: [], actions: [] });
  /** The panel is listing the buffer's actions rather than the row's. */
  let bufferPanel = $state(false);
  /** Move to… / Copy to… is waiting for a folder to be typed or picked. */
  let destination = $state<{ key: string; label: string } | null>(null);
  /** A buffer action is running (the shell does one at a time). */
  let bufferBusy = $state(false);
  let bufferProgress = $state<BufferProgress | null>(null);
  /** How the last buffer action ended. */
  let bufferNote = $state<BufferNote | null>(null);

  interface PanelEntry {
    label: string;
    /** Muted text after the label: a preview of what the action produces. */
    detail?: string;
    modifier: Modifier | null;
    /** Index into the row's secondary actions; `null` is the primary action. */
    index: number | null;
    /** What Enter does, as a word (Universal Actions). */
    verb?: string;
    /** The other modifier + Enter shortcuts of the action (Universal Actions). */
    extras?: { modifier: Modifier; label: string }[];
    /** A file buffer action, by key. */
    bufferKey?: string;
    /** Entries that are not a result action: collect the row, open the buffer's actions, ... */
    special?: "buffer_add" | "buffer_actions" | "buffer_more";
  }

  /** A row of the file results (not bookmarks or apps): what can be collected. */
  function isFileRow(item: ResultDto | undefined): item is ResultDto {
    return !!item && item.plugin_id === "files" && item.action === "open_path";
  }

  /** A folder row of a browsed path: the rows Move to… / Copy to… can choose. */
  function isFolderRow(item: ResultDto): boolean {
    return isFileRow(item) && /[\\/]$/.test(item.autocomplete ?? "");
  }

  /** While a destination is wanted, only folders are worth listing. */
  function forDestination(found: ResultDto[]): ResultDto[] {
    return destination ? found.filter(isFolderRow) : found;
  }

  const current = $derived<ResultDto | undefined>(results[selected]);
  const mac = $derived(status?.display === "macos");
  const altKey = $derived(mac ? "⌥" : "Alt+");
  const panelEntries = $derived<PanelEntry[]>(
    selection
      ? selection.actions.map((action) => ({
          label: action.title,
          detail: action.subtitle,
          modifier: null,
          index: null,
          verb: selectionVerb(action),
          extras: action.secondary.flatMap((s) =>
            s.modifier ? [{ modifier: s.modifier, label: s.label }] : [],
          ),
        }))
      : bufferPanel
        ? [
            ...fileBuffer.actions.map((action) => ({
              label: action.label,
              detail: action.destination ? "then choose a folder" : undefined,
              modifier: null,
              index: null,
              bufferKey: action.key,
            })),
            {
              label: "More file actions…",
              detail: "open with, reveal, share…",
              modifier: null,
              index: null,
              special: "buffer_more" as const,
            },
          ]
        : current
          ? [
              { label: verb(current), modifier: null, index: null },
              ...current.secondary.map((s, index) => ({
                label: s.label,
                modifier: s.modifier,
                index,
              })),
              ...(isFileRow(current)
                ? [
                    {
                      label: "Add to file buffer",
                      detail: `${altKey}↑ or ${altKey}↓`,
                      modifier: null,
                      index: null,
                      special: "buffer_add" as const,
                    },
                  ]
                : []),
              ...(fileBuffer.items.length > 0
                ? [
                    {
                      label: `File buffer actions (${fileBuffer.items.length})…`,
                      detail: `${altKey}→`,
                      modifier: null,
                      index: null,
                      special: "buffer_actions" as const,
                    },
                  ]
                : []),
            ]
          : [],
  );
  const modifierHints = $derived(current?.secondary.filter((s) => s.modifier !== null) ?? []);
  /** Every result asks to be a tile: show them as a grid instead of a list. */
  const gridMode = $derived(results.length > 0 && results.every((result) => result.tile));
  /** The pane is shown (it gives way to the action panel, Large Type and the Text View). */
  const paneVisible = $derived(
    previewOpen && !!current && !panelOpen && largeText === null && !textViewOpen,
  );

  const hotkeyError = $derived(status?.hotkey.error ?? null);
  const accelerator = $derived(status?.hotkey.accelerator ?? "");
  /** The shortcut by this OS's names for the keys (Win+Space, Cmd+Space...). */
  const shownAccelerator = $derived(
    displayAccelerator(
      accelerator,
      status?.display === "windows" ? "windows" : status?.display === "macos" ? "macos" : "linux",
    ),
  );
  const hasQuery = $derived(query.trim() !== "");
  // While the index is still filling, a query often yields nothing but the
  // "search the web" fallback; say why instead of looking like a miss.
  const showIndexing = $derived(
    indexing &&
      hasQuery &&
      !error &&
      results.every((result) => result.plugin_id.startsWith("web:")),
  );

  // The buffer's action list only exists while the panel is open.
  $effect(() => {
    if (!panelOpen) bufferPanel = false;
  });

  let lastHeight = -1;
  /** Tags every search; a response is applied only if it is still the latest. */
  let searchSeq = 0;
  let executing = false;
  /** Executed queries (newest first) while Up/Down recall them; empty otherwise. */
  let history: string[] = [];
  /** Index into `history` of the query shown, or -1 when not recalling. */
  let historyPos = -1;
  let historyQueue: Promise<void> = Promise.resolve();
  let lastPointer = { x: -1, y: -1 };

  function focusInput(select = false) {
    if (!input) return;
    input.focus();
    if (select) input.select();
  }

  function reportHeight() {
    // The stretched window is Large Type's; the launcher's height comes back
    // when it is dismissed.
    if (!shell || largeFull) return;
    const h = Math.ceil(shell.offsetHeight);
    if (h <= 0 || h === lastHeight) return;
    lastHeight = h;
    void setContentHeight(h);
  }

  function reset() {
    searchSeq++; // drop any search still in flight
    panelOpen = false;
    selection = null;
    destination = null;
    bufferNote = null;
    outputView = null;
    closeLargeType();
    previewOpen = false;
    previewContent = null;
    closeTextView(false);
    historyPos = -1;
    query = "";
    results = [];
    selected = 0;
    error = null;
    brokenIcons = {};
  }

  /** The window is being shown; `payload` can prefill the query or carry an error. */
  function applyShow(payload: ShowPayload | null) {
    playOpen();
    reset();
    void refreshBuffer();
    if (payload?.selection) {
      openSelection(payload.selection);
      return;
    }
    if (payload?.output) {
      showOutput(payload.output);
      return;
    }
    if (payload?.error) error = payload.error;
    if (payload?.query) {
      prefill(payload.query);
      return;
    }
    focusInput(true);
  }

  /** Restarts the short fade-and-settle animation of the card (see `.opening`). */
  function playOpen() {
    opening = false;
    requestAnimationFrame(() => (opening = true));
  }

  /** Shows what a workflow's Large Type or text view node produced. */
  function showOutput(output: OutputPayload) {
    if (output.kind === "large_type") {
      void openLargeType(output.text);
      return;
    }
    outputView = { heading: output.heading, text: output.text };
    focusInput(true);
  }

  /** Puts `text` in the search box and searches for it, caret at the end. */
  function prefill(text: string) {
    query = text;
    void runSearch(text);
    // Caret at the end, so typing continues after the prefilled text.
    void tick().then(() => {
      focusInput();
      input?.setSelectionRange(text.length, text.length);
    });
  }

  /** Shows the actions for the selection that was captured in another app. */
  function openSelection(payload: SelectionPayload) {
    selection = payload;
    panelIndex = 0;
    panelOpen = true;
    focusInput(true);
  }

  /** Back to ordinary searching: the selection's actions are gone. */
  function leaveSelection() {
    selection = null;
    panelOpen = false;
  }

  /** What Enter does for a Universal Actions entry, as one word. */
  function selectionVerb(action: SelectionActionDto): string {
    if (action.window?.kind === "large_type") return "Show";
    if (action.window?.kind === "search") return "Browse";
    if (action.action === "paste_text") return "Replace";
    return verb(action);
  }

  async function runSearch(text: string) {
    const mine = ++searchSeq;
    if (text.trim() === "") {
      results = [];
      selected = 0;
      return;
    }
    const found = await search(text);
    if (mine !== searchSeq || found === null) return;
    results = forDestination(found.results);
    resultsTicket = found.ticket;
    selected = 0;
    panelOpen = false;
    if (list) list.scrollTop = 0;
  }

  /**
   * A script plugin answered after its query returned: run the query on screen
   * again. Unlike a fresh search this keeps the selected row if it is still there.
   */
  async function refreshResults() {
    const text = query;
    if (text.trim() === "") return;
    const mine = ++searchSeq;
    const found = await search(text);
    if (mine !== searchSeq || found === null) return;
    const keep = results[selected]?.id;
    results = forDestination(found.results);
    resultsTicket = found.ticket;
    const at = keep === undefined ? -1 : results.findIndex((result) => result.id === keep);
    if (at >= 0) {
      selected = at;
      // An open action panel stays on its row; its list may have changed.
      if (panelIndex >= panelEntries.length) panelIndex = 0;
    } else {
      selected = 0;
      // The row the panel was for is gone.
      panelOpen = false;
    }
    // History recall, Tab completion and Large Type are left as they are.
  }

  function onInput(e: Event) {
    error = null;
    historyPos = -1; // typing leaves history recall
    void runSearch((e.currentTarget as HTMLInputElement).value);
  }

  /** Replaces the input (Tab completion, history recall) and searches for it. */
  function setQuery(text: string) {
    error = null;
    query = text;
    void runSearch(text);
  }

  /**
   * Up/Down on an empty input walk back and forward through executed queries.
   * Down past the newest one returns to the empty input.
   */
  function stepHistory(older: boolean) {
    // One step at a time, so a quick second key press waits for the first
    // press to finish loading the history instead of being lost.
    historyQueue = historyQueue.then(() => stepHistoryNow(older));
  }

  async function stepHistoryNow(older: boolean) {
    if (historyPos < 0) {
      if (!older) return;
      const loaded = await queryHistory();
      // The user may have typed or recalled while this was loading.
      if (query !== "" || historyPos >= 0 || loaded.length === 0) return;
      history = loaded;
      historyPos = 0;
    } else {
      const next = historyPos + (older ? 1 : -1);
      if (next >= history.length) return;
      historyPos = next;
      if (next < 0) {
        setQuery("");
        return;
      }
    }
    setQuery(history[historyPos]);
  }

  /** Tab completes to the selected row's suggestion; Shift+Tab goes up a folder. */
  function complete(up: boolean) {
    historyPos = -1;
    const text = up ? parentPath(query) : results[selected]?.autocomplete;
    if (text && text !== query) setQuery(text);
  }

  /** Runs row `index`: its primary action, or secondary action number `action`. */
  async function run(index: number, action?: number) {
    if (destination) {
      void chooseDestination(index, false);
      return;
    }
    const item = results[index];
    if (!item || executing) return;
    executing = true;
    try {
      const message = await execute(item.id, resultsTicket, action);
      if (message !== null) error = message;
    } finally {
      executing = false;
    }
  }

  /** The key held with Enter, if it is exactly one of Ctrl (Cmd), Shift or Alt. */
  function modifierOf(e: KeyboardEvent): Modifier | null {
    const ctrl = e.ctrlKey || e.metaKey;
    if (ctrl && !e.shiftKey && !e.altKey) return "ctrl";
    if (e.shiftKey && !ctrl && !e.altKey) return "shift";
    if (e.altKey && !ctrl && !e.shiftKey) return "alt";
    return null;
  }

  /** Enter with a modifier: run the selected row's secondary action bound to it. */
  function runModified(modifier: Modifier) {
    const action = current?.secondary.findIndex((s) => s.modifier === modifier) ?? -1;
    if (action >= 0) void run(selected, action);
  }

  function openPanel() {
    if (!current || executing) return;
    panelIndex = 0;
    panelOpen = true;
  }

  /** `modifier` (Universal Actions only) picks the entry's alternative action. */
  function runPanelEntry(k: number, modifier: Modifier | null = null) {
    const entry = panelEntries[k];
    if (!entry) return;
    if (selection) {
      void runSelection(selection.actions[k], modifier);
      return;
    }
    if (entry.special === "buffer_actions") {
      openBufferPanel();
      return;
    }
    if (entry.special === "buffer_more") {
      void moreFileActions();
      return;
    }
    panelOpen = false;
    if (entry.special === "buffer_add") {
      void collect(0);
      return;
    }
    if (entry.bufferKey) {
      void startBufferAction(entry.bufferKey);
      return;
    }
    void run(selected, entry.index ?? undefined);
  }

  /** Runs a Universal Actions entry: the window's own, or the shell's. */
  async function runSelection(action: SelectionActionDto | undefined, modifier: Modifier | null) {
    const set = selection;
    if (!action || !set || executing) return;
    let alternative: number | undefined;
    if (modifier !== null) {
      // An action without that alternative does nothing for the key.
      alternative = action.secondary.findIndex((s) => s.modifier === modifier);
      if (alternative < 0) return;
    } else if (action.window) {
      runWindowAction(action.window);
      return;
    }
    executing = true;
    try {
      const message = await execute(action.id, set.ticket, alternative);
      if (message !== null) error = message;
    } finally {
      executing = false;
    }
  }

  function runWindowAction(action: WindowAction) {
    if (action.kind === "large_type") {
      void openLargeType(action.text);
    } else {
      leaveSelection();
      prefill(action.query);
    }
  }

  async function refreshBuffer() {
    const found = await fileBufferGet();
    if (found) fileBuffer = found;
  }

  /**
   * Alt+Up / Alt+Down: the selected file or folder goes into the buffer, and the
   * selection moves on (`delta` 0 stays, for the action panel's entry).
   */
  async function collect(delta: number) {
    const item = current;
    const ticket = resultsTicket;
    if (!isFileRow(item) || bufferBusy) return;
    bufferNote = null;
    if (delta !== 0) move(delta);
    const found = await fileBufferAdd(item.id, ticket);
    if (typeof found === "string") error = found;
    else fileBuffer = found;
  }

  async function removeFromBuffer(index?: number) {
    if (bufferBusy) return;
    bufferNote = null;
    const found = await fileBufferRemove(index);
    if (found) fileBuffer = found;
  }

  async function clearBuffer() {
    if (bufferBusy) return;
    bufferNote = null;
    const found = await fileBufferClear();
    if (found) fileBuffer = found;
  }

  function openBufferPanel() {
    if (fileBuffer.items.length === 0 || bufferBusy) return;
    selection = null;
    bufferPanel = true;
    panelIndex = 0;
    panelOpen = true;
  }

  /**
   * The Alt+arrow keys of the file buffer. Returns false for a key that is not
   * the buffer's to handle right now (so the input keeps it).
   */
  function onBufferKey(e: KeyboardEvent): boolean {
    if (destination) return false;
    const filled = fileBuffer.items.length > 0;
    switch (e.key) {
      case "ArrowUp":
      case "ArrowDown":
        if (!isFileRow(current)) return false;
        void collect(e.key === "ArrowUp" ? -1 : 1);
        return true;
      case "ArrowLeft":
        if (!filled) return false;
        void removeFromBuffer();
        return true;
      case "ArrowRight":
        if (!filled) return false;
        openBufferPanel();
        return true;
      case "Backspace":
      case "Delete":
        if (!filled) return false;
        void clearBuffer();
        return true;
      default:
        return false;
    }
  }

  /** A picked buffer action: ask for a folder first when it needs one. */
  async function startBufferAction(key: string) {
    const action = fileBuffer.actions.find((a) => a.key === key);
    if (!action || bufferBusy) return;
    bufferNote = null;
    error = null;
    if (action.destination) {
      destination = { key, label: action.label };
      historyPos = -1;
      setQuery("~/");
      focusInput();
      return;
    }
    await runBufferAction(key, null);
  }

  /**
   * Runs a buffer action in the shell. Resolves to false when it did not run
   * (an error, or the confirmation was declined).
   */
  async function runBufferAction(key: string, to: BufferDestination | null): Promise<boolean> {
    if (bufferBusy) return false;
    bufferBusy = true;
    bufferNote = null;
    bufferProgress = null;
    try {
      const outcome = await fileBufferRun(key, to);
      if (typeof outcome === "string") {
        error = outcome;
        return false;
      }
      fileBuffer = outcome.buffer;
      if (outcome.declined) return false;
      if (!outcome.hidden && outcome.message) {
        bufferNote = { text: outcome.message, error: !outcome.ok };
      }
      return true;
    } finally {
      bufferBusy = false;
      bufferProgress = null;
    }
  }

  /**
   * Move to… / Copy to… got its folder: the highlighted folder row, or with
   * `typed` the path as typed (a keyword in front of it is ignored).
   */
  async function chooseDestination(index: number, typed: boolean) {
    const wanted = destination;
    if (!wanted || bufferBusy) return;
    const row = results[index];
    const path = query.trim().replace(/^\S+\s+(?=(~[\\/]|[\\/]|[A-Za-z]:[\\/]))/, "");
    let to: BufferDestination;
    if (!typed && row && isFolderRow(row)) {
      to = { kind: "result", id: row.id, ticket: resultsTicket };
    } else if (path !== "") {
      to = { kind: "text", text: path };
    } else {
      error = "Pick a folder first.";
      return;
    }
    error = null;
    if (await runBufferAction(wanted.key, to)) leaveDestination();
  }

  function leaveDestination() {
    destination = null;
    setQuery("");
  }

  /** The Universal Actions list for the collected files. */
  async function moreFileActions() {
    const found = await fileBufferSelection();
    if (typeof found === "string") {
      error = found;
      leaveSelection();
      return;
    }
    openSelection(found);
  }

  function onPanelMove(e: MouseEvent, k: number) {
    if (e.movementX === 0 && e.movementY === 0) return;
    panelIndex = k;
  }

  /** Ctrl+C with nothing selected in the input copies the row's path, URL or value. */
  async function copySelected() {
    const item = current;
    if (!item?.copy_text || executing) return;
    executing = true;
    try {
      const message = await copyResult(item.id, resultsTicket);
      if (message !== null) error = message;
    } finally {
      executing = false;
    }
  }

  /** Shift tap / Ctrl+Y: show or hide the preview pane for the selected row. */
  function togglePreview() {
    if (selection || largeText !== null || textViewOpen) return;
    if (previewOpen) {
      previewOpen = false;
    } else if (current) {
      previewOpen = true;
    }
  }

  // The pane follows the selection. A short delay keeps a held arrow key from
  // reading a file for every row it passes over; the old content stays on
  // screen (dimmed) until the new one arrives.
  $effect(() => {
    if (!previewOpen || !current) return;
    const id = current.id;
    const ticket = resultsTicket;
    const mine = ++previewSeq;
    previewLoading = true;
    const timer = setTimeout(() => {
      void preview(id, ticket).then((content) => {
        if (mine !== previewSeq) return;
        previewContent = content;
        previewLoading = false;
      });
    }, 60);
    return () => clearTimeout(timer);
  });

  /** Ctrl+T (or Enter on a text-only row): the row's long text in the Text View. */
  async function openTextView() {
    const item = current;
    if (!item?.text_view || selection) return;
    const mine = ++textSeq;
    textViewOpen = true;
    textContent = null;
    panelOpen = false;
    const text = await textView(item.id, resultsTicket);
    if (mine !== textSeq) return;
    textContent = text ?? { title: item.title, text: "There is nothing to show.", truncated: false };
  }

  /** Back to the list (Esc or Left); `refocus` puts the caret back in the search box. */
  function closeTextView(refocus = true) {
    textSeq++;
    if (!textViewOpen) return;
    textViewOpen = false;
    textContent = null;
    if (refocus) void tick().then(() => focusInput());
  }

  /** Arrow keys, PageUp/PageDown, Home and End scroll the Text View. */
  function scrollTextView(key: string): boolean {
    switch (key) {
      case "ArrowDown":
        textViewEl?.scrollBy(48);
        return true;
      case "ArrowUp":
        textViewEl?.scrollBy(-48);
        return true;
      case "PageDown":
        textViewEl?.scrollBy("page");
        return true;
      case "PageUp":
        textViewEl?.scrollBy("-page");
        return true;
      case "Home":
        textViewEl?.scrollToEdge(false);
        return true;
      case "End":
        textViewEl?.scrollToEdge(true);
        return true;
      default:
        return false;
    }
  }

  /** Grid View arrows: moves by whole rows (`dy`) or tiles (`dx`), staying inside the grid. */
  function gridMove(dx: number, dy: number) {
    const n = results.length;
    if (n === 0) return;
    let next = selected + dx + dy * gridCols;
    if (dy > 0 && next >= n) {
      // A short last row: Down from the row above lands on its last tile.
      const lastRow = Math.floor((n - 1) / gridCols) * gridCols;
      next = selected >= lastRow ? selected : n - 1;
    }
    void select(Math.max(0, Math.min(n - 1, next)), true);
  }

  async function openLargeType(text: string) {
    const mine = ++largeSeq;
    // The selection's actions come back when Large Type is dismissed.
    if (!selection) panelOpen = false;
    largeText = text;
    const full = await setLargeType(true);
    // The webview learns its new size a moment after the window has it.
    if (full) await new Promise((resolve) => setTimeout(resolve, 80));
    if (mine !== largeSeq) return; // dismissed meanwhile; closing restored the window
    largeFull = full;
    // Size the text to the room there is: roughly one em-square of 0.75 em²
    // per character, over most of the usable area.
    const width = (full ? window.innerWidth * 0.8 : window.innerWidth - 80) || 600;
    const height = (full ? window.innerHeight * 0.65 : 180) || 300;
    const fit = Math.sqrt((width * height) / (Math.max(text.length, 1) * 0.75));
    largeFont = Math.round(Math.max(28, Math.min(fit, height * 0.8, 420)));
    largeReady = true;
  }

  function closeLargeType() {
    if (largeText === null) return;
    largeSeq++;
    largeText = null;
    largeFull = false;
    largeReady = false;
    // Always: the window may have been stretched after the user already left.
    void setLargeType(false);
    lastHeight = -1;
    void tick().then(() => {
      reportHeight();
      focusInput();
    });
  }

  /** Plain Right arrow with the caret at the end: it would not move anything. */
  function rightArrowIsFree(e: KeyboardEvent): boolean {
    return (
      !!input &&
      !e.shiftKey &&
      !e.ctrlKey &&
      !e.altKey &&
      !e.metaKey &&
      input.selectionStart === input.value.length &&
      input.selectionEnd === input.value.length
    );
  }

  /** The key hint for a modifier + Enter, as drawn on the keyboard. */
  function combo(modifier: Modifier): string {
    if (mac) return { ctrl: "⌘", shift: "⇧", alt: "⌥" }[modifier] + "↵";
    return { ctrl: "Ctrl", shift: "Shift", alt: "Alt" }[modifier] + "+↵";
  }

  /** Moves the selection; `reveal` scrolls it into view (keyboard only). */
  async function select(index: number, reveal = false) {
    if (results.length === 0) return;
    selected = index;
    if (!reveal) return;
    await tick();
    if (gridMode) {
      document.getElementById(`result-${selected}`)?.scrollIntoView({ block: "nearest" });
      return;
    }
    const row = list?.children[selected] as HTMLElement | undefined;
    if (!list || !row) return;
    if (selected === 0) list.scrollTop = 0;
    else if (selected === results.length - 1) list.scrollTop = list.scrollHeight;
    else if (row.offsetTop < list.scrollTop) list.scrollTop = row.offsetTop;
    else if (row.offsetTop + row.offsetHeight > list.scrollTop + list.clientHeight) {
      list.scrollTop = row.offsetTop + row.offsetHeight - list.clientHeight;
    }
  }

  function move(delta: number) {
    const n = results.length;
    if (n > 0) void select((selected + delta + n) % n, true);
  }

  function page(delta: number) {
    const n = results.length;
    if (n > 0) void select(Math.min(n - 1, Math.max(0, selected + delta)), true);
  }

  /** Shift released soon after it went down alone: a tap, which toggles the preview. */
  function onKeyup(e: KeyboardEvent) {
    if (e.key !== "Shift") return;
    const tapped = shiftDownAt > 0 && performance.now() - shiftDownAt < 450;
    shiftDownAt = 0;
    if (tapped && !e.isComposing) togglePreview();
  }

  function onKeydown(e: KeyboardEvent) {
    // A lone Shift is a candidate tap. Any other key meanwhile (Shift+Enter, a
    // capital letter, Alt+Shift switching the keyboard layout) or key repeat
    // (holding Shift) makes it not one.
    shiftDownAt =
      e.key === "Shift" && !e.repeat && !e.ctrlKey && !e.altKey && !e.metaKey
        ? performance.now()
        : 0;
    if (e.isComposing) return;
    const key = e.key;
    const ctrl = (e.ctrlKey || e.metaKey) && !e.altKey;
    const lower = key.toLowerCase();

    if (largeText !== null) {
      // Any key dismisses Large Type, except the modifier that was still down
      // from Ctrl+L and key repeat of the opening shortcut.
      if (e.repeat || ["Control", "Shift", "Alt", "Meta"].includes(key)) return;
      e.preventDefault();
      closeLargeType();
      return;
    }

    if (textViewOpen) {
      if (key === "Escape" || (key === "ArrowLeft" && !e.shiftKey) || (ctrl && lower === "t")) {
        e.preventDefault();
        closeTextView();
        return;
      } else if (!ctrl && !e.altKey && !e.shiftKey && scrollTextView(key)) {
        e.preventDefault();
        return;
      } else if (key === "Enter") {
        e.preventDefault();
        const modifier = modifierOf(e);
        if (modifier === null) void run(selected);
        else runModified(modifier);
        return;
      } else if (ctrl && !e.shiftKey && lower === "c") {
        // Text the user selected with the mouse copies as usual; otherwise the
        // row's text goes to the clipboard.
        if (!window.getSelection()?.toString()) {
          e.preventDefault();
          void copySelected();
        }
        return;
      } else if (["Control", "Shift", "Alt", "Meta"].includes(key)) {
        return;
      }
      // Typing and the like go back to the list and act as usual.
      closeTextView();
    }

    if (panelOpen) {
      const n = panelEntries.length;
      if (key === "Escape" && selection) {
        // Nothing else is behind the actions: Esc dismisses the launcher.
        e.preventDefault();
        void hideWindow();
        return;
      } else if (key === "Escape" || key === "ArrowLeft" || (ctrl && lower === "k")) {
        e.preventDefault();
        leaveSelection();
        return;
      } else if (key === "ArrowDown" || (ctrl && lower === "n")) {
        e.preventDefault();
        panelIndex = (panelIndex + 1) % n;
        return;
      } else if (key === "ArrowUp" || (ctrl && lower === "p")) {
        e.preventDefault();
        panelIndex = (panelIndex - 1 + n) % n;
        return;
      } else if (key === "Enter") {
        e.preventDefault();
        runPanelEntry(panelIndex, selection ? modifierOf(e) : null);
        return;
      } else if (selection && ctrl && /^[1-9]$/.test(key)) {
        e.preventDefault();
        runPanelEntry(Number(key) - 1);
        return;
      } else if (["Control", "Shift", "Alt", "Meta"].includes(key)) {
        return;
      }
      // Anything else (typing, Ctrl+1, ...) closes the panel and acts as usual.
      leaveSelection();
    }

    if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && onBufferKey(e)) {
      e.preventDefault();
      return;
    }
    // Esc closes the innermost thing first: an open preview pane, then the
    // folder picker of Move to… / Copy to…, then the launcher.
    if (destination && key === "Escape" && !previewOpen) {
      e.preventDefault();
      leaveDestination();
      return;
    }
    if (destination && key === "Enter") {
      e.preventDefault();
      void chooseDestination(selected, e.ctrlKey || e.metaKey);
      return;
    }

    if (key === "Escape") {
      e.preventDefault();
      // The preview pane, then a workflow's text output, close first; the
      // next Esc hides the launcher.
      if (previewOpen) previewOpen = false;
      else if (outputView) outputView = null;
      else void hideWindow();
    } else if (ctrl && !e.shiftKey && lower === "y") {
      e.preventDefault();
      if (!e.repeat) togglePreview();
    } else if (ctrl && !e.shiftKey && lower === "t") {
      e.preventDefault();
      if (!e.repeat) void openTextView();
    } else if (
      gridMode &&
      (key === "ArrowLeft" || key === "ArrowRight") &&
      !ctrl &&
      !e.altKey &&
      !e.shiftKey
    ) {
      // In a grid the sideways arrows walk the tiles (Ctrl+K opens the actions).
      e.preventDefault();
      gridMove(key === "ArrowRight" ? 1 : -1, 0);
    } else if (key === "ArrowRight" && current && rightArrowIsFree(e)) {
      e.preventDefault();
      openPanel();
    } else if (ctrl && !e.shiftKey && lower === "k") {
      e.preventDefault();
      openPanel();
    } else if (ctrl && !e.shiftKey && lower === "l") {
      e.preventDefault();
      if (current && !e.repeat) void openLargeType(current.large_text ?? current.title);
    } else if (
      ctrl &&
      !e.shiftKey &&
      lower === "c" &&
      current?.copy_text &&
      input?.selectionStart === input?.selectionEnd
    ) {
      // Nothing is selected in the input, so there is no text for the browser
      // to copy; copy the row instead.
      e.preventDefault();
      void copySelected();
    } else if (
      (key === "ArrowUp" || key === "ArrowDown") &&
      !ctrl &&
      !e.altKey &&
      !e.shiftKey &&
      (historyPos >= 0 || query === "")
    ) {
      e.preventDefault();
      stepHistory(key === "ArrowUp");
    } else if (key === "Tab" && !ctrl && !e.altKey) {
      e.preventDefault(); // the input is the only focus target; keep it
      complete(e.shiftKey);
    } else if (key === "ArrowDown" || (ctrl && lower === "n")) {
      e.preventDefault();
      if (gridMode) gridMove(0, 1);
      else move(1);
    } else if (key === "ArrowUp" || (ctrl && lower === "p")) {
      e.preventDefault();
      if (gridMode) gridMove(0, -1);
      else move(-1);
    } else if (key === "PageDown") {
      e.preventDefault();
      if (gridMode) gridMove(0, 3);
      else page(7);
    } else if (key === "PageUp") {
      e.preventDefault();
      if (gridMode) gridMove(0, -3);
      else page(-7);
    } else if (key === "Enter") {
      e.preventDefault();
      const modifier = modifierOf(e);
      if (modifier === null && current?.text_on_enter) void openTextView();
      else if (modifier === null) void run(selected);
      else runModified(modifier);
    } else if (ctrl && /^[1-9]$/.test(key)) {
      e.preventDefault();
      void run(Number(key) - 1);
    } else if (
      import.meta.env.PROD &&
      (key === "F5" || (ctrl && lower === "r") || (ctrl && lower === "f"))
    ) {
      e.preventDefault();
    }
  }

  function onRowMove(e: MouseEvent, index: number) {
    // Browsers synthesize mousemove when content moves under a stationary
    // pointer (after a re-render, a scroll, or the window appearing). Those
    // carry no movement; only a real pointer movement may change the selection.
    if (e.movementX === 0 && e.movementY === 0) return;
    if (e.screenX === lastPointer.x && e.screenY === lastPointer.y) return;
    lastPointer = { x: e.screenX, y: e.screenY };
    if (index !== selected) selected = index;
  }

  function onContextMenu(e: MouseEvent) {
    if (import.meta.env.PROD) e.preventDefault();
  }

  function verb(item: ResultDto): string {
    if (destination && isFolderRow(item)) {
      return destination.label.startsWith("Move") ? "Move here" : "Copy here";
    }
    if (item.plugin_id.startsWith("web:")) return "Search";
    switch (item.action) {
      case "launch":
        return "Launch";
      case "copy_text":
        return "Copy";
      case "paste_text":
        return "Paste";
      case "custom":
        return "Run";
      case "reveal_path":
        return "Reveal";
      case "run_as_admin":
        return "Run as admin";
      default:
        return "Open";
    }
  }

  /** The glyph for rows without a (working) image. */
  function glyphFor(item: ResultDto): string {
    if (item.icon?.kind === "builtin") return item.icon.name;
    return item.action === "launch" ? "app" : "file";
  }

  // Keep the entry the arrow keys moved to in view (the selection's list scrolls).
  $effect(() => {
    void panelIndex;
    if (!panelOpen || !panelEl) return;
    const panel = panelEl;
    void tick().then(() =>
      panel.querySelector<HTMLElement>(".action.selected")?.scrollIntoView({ block: "nearest" }),
    );
  });

  onMount(() => {
    focusInput(true);
    void refreshBuffer();

    const resizeObserver = new ResizeObserver(reportHeight);
    if (shell) resizeObserver.observe(shell);
    reportHeight();

    getStatus().then((s) => {
      if (s) {
        status = s;
        indexing = s.indexing;
        applyTheme(s.theme);
        applyAppearance(s.appearance);
      }
    });

    const unlisteners = [
      onShow(applyShow),
      onHidden(reset),
      onStatus((s) => {
        status = s;
        indexing = s.indexing;
        applyTheme(s.theme);
        applyAppearance(s.appearance);
      }),
      onIndex((state) => {
        indexing = state === "indexing";
      }),
      onResultsUpdated(() => void refreshResults()),
      onBufferProgress((progress) => {
        if (bufferBusy) bufferProgress = progress;
      }),
    ];

    // Browser preview only: `/#selection` shows the Universal Actions panel.
    if (import.meta.env.DEV && !hasTauri() && location.hash === "#selection") {
      void import("./lib/mock").then(({ mockSelection }) => openSelection(mockSelection()));
    }

    // Browser preview only: `/#buffer` starts with some collected files, and
    // `/#buffer-dest` with Move to… waiting for a folder.
    if (import.meta.env.DEV && !hasTauri() && location.hash.startsWith("#buffer")) {
      void import("./lib/mock").then(async ({ mockBuffer }) => {
        fileBuffer = mockBuffer.seed();
        setQuery("report");
        if (location.hash === "#buffer-dest") await startBufferAction("move_to");
      });
    }

    // A query sent while the window was still loading (`sevak --query` at startup).
    void Promise.all(unlisteners)
      .then(() => takePendingShow())
      .then((payload) => {
        if (payload) applyShow(payload);
      });

    return () => {
      resizeObserver.disconnect();
      for (const u of unlisteners) void u.then((fn) => fn());
    };
  });
</script>

<svelte:window onkeydown={onKeydown} onkeyup={onKeyup} onfocus={() => focusInput()} oncontextmenu={onContextMenu} />

<main class="shell" bind:this={shell}>
  <div class="card" class:covered={largeFull} class:opening>
    <div class="bar">
      <img
        class="brand-icon"
        src="/sevak-icon.png"
        width="28"
        height="28"
        alt="Sevak"
        draggable="false"
      />
      <input
        bind:this={input}
        bind:value={query}
        oninput={onInput}
        type="text"
        role="combobox"
        placeholder="Type to search…"
        spellcheck="false"
        autocomplete="off"
        autocorrect="off"
        autocapitalize="off"
        aria-label="Search"
        aria-autocomplete="list"
        aria-expanded={results.length > 0}
        aria-controls="results"
        aria-activedescendant={results.length > 0 ? `result-${selected}` : undefined}
      />
    </div>

    {#if hotkeyError}
      <div class="notice" role="status">
        Shortcut "{shownAccelerator}" is unavailable: {hotkeyError}. Choose another one in
        Settings (tray menu).
      </div>
    {/if}

    <FileBufferStrip
      items={fileBuffer.items}
      progress={bufferProgress}
      note={bufferNote}
      destination={destination && { label: destination.label, count: fileBuffer.items.length }}
      {mac}
      onremove={(index) => void removeFromBuffer(index)}
    />

    {#if gridMode && !textViewOpen}
      <GridView
        items={results}
        {selected}
        verb={current ? verb(current) : ""}
        compact={paneVisible}
        bind:columns={gridCols}
        onhover={(i) => (selected = i)}
        onrun={(i) => void run(i)}
      />
    {:else if !gridMode}
    <div
      id="results"
      class="results"
      class:empty={results.length === 0}
      class:compact={paneVisible}
      class:hidden={textViewOpen}
      role="listbox"
      aria-label="Results"
      bind:this={list}
    >
      {#each results as item, i (item.id)}
        <!-- The row itself lives in lib/ResultRow.svelte, shared with the theme editor's preview. -->
        {@const iconUrl = item.icon?.kind === "url" ? item.icon.url : null}
        <ResultRow
          id="result-{i}"
          {item}
          index={i}
          selected={i === selected}
          glyph={glyphFor(item)}
          verb={verb(item)}
          broken={iconUrl !== null && !!brokenIcons[iconUrl]}
          onbroken={() => iconUrl !== null && (brokenIcons[iconUrl] = true)}
          onmove={(e) => onRowMove(e, i)}
          onrun={() => void run(i)}
        />
      {/each}
    </div>
    {/if}

    {#if textViewOpen}
      <TextView bind:this={textViewEl} content={textContent} {mac} />
    {:else if paneVisible}
      <PreviewPane
        content={previewContent}
        loading={previewLoading}
        glyph={current?.glyph ?? null}
        hasTextView={!!current?.text_view}
        {mac}
      />
    {/if}

    {#if panelOpen && largeText === null && (selection || bufferPanel || current)}
      <div
        class="panel"
        class:scroll={!!selection || bufferPanel}
        role="menu"
        aria-label="Actions for {selection
          ? selection.title
          : bufferPanel
            ? 'the file buffer'
            : current?.title}"
        bind:this={panelEl}
      >
        <div class="panel-title">
          {#if selection}
            {selection.title}{#if selection.subtitle}: <span class="quote">{selection.subtitle}</span>{/if}
          {:else if bufferPanel}
            File buffer: {fileBuffer.items.length === 1 ? "1 item" : `${fileBuffer.items.length} items`}
          {:else}
            Actions for {current?.title}
          {/if}
        </div>
        {#each panelEntries as entry, k (k)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="action"
            class:selected={k === panelIndex}
            role="menuitem"
            tabindex="-1"
            onmousemove={(e) => onPanelMove(e, k)}
            onmousedown={(e) => e.preventDefault()}
            onclick={() => runPanelEntry(k)}
          >
            <span class="label">{entry.label}</span>
            {#if entry.detail}<span class="detail">{entry.detail}</span>{/if}
            <span class="keys">
              {#each entry.extras ?? [] as extra (extra.modifier)}
                <span class="chip"><kbd>{combo(extra.modifier)}</kbd>{extra.label}</span>
              {/each}
              <span class="chip"
                ><kbd>{entry.modifier ? combo(entry.modifier) : "↵"}</kbd>{entry.verb ?? ""}</span
              >
            </span>
          </div>
        {/each}
      </div>
    {:else if current && !textViewOpen && !destination && (current.secondary.length > 0 || current.autocomplete)}
      <div class="hints" aria-hidden="true">
        <span class="chips">
          {#if current.autocomplete}
            <span class="chip"><kbd>Tab</kbd>Complete</span>
          {/if}
          {#each modifierHints as hint (hint.label)}
            {#if hint.modifier}
              <span class="chip"><kbd>{combo(hint.modifier)}</kbd>{hint.label}</span>
            {/if}
          {/each}
          {#if isFileRow(current) && !destination}
            <span class="chip"><kbd>{altKey}↑↓</kbd>Collect</span>
          {/if}
        </span>
        {#if current.secondary.length > 0}
          <span class="chip more"><kbd>→</kbd>Actions</span>
        {/if}
      </div>
    {/if}

    {#if outputView}
      <div class="output" role="region" aria-label={outputView.heading || "Workflow output"}>
        {#if outputView.heading}<div class="output-heading">{outputView.heading}</div>{/if}
        <pre class="output-text">{outputView.text}</pre>
      </div>
    {/if}

    {#if error}
      <div class="error" role="alert">{error}</div>
    {/if}
    {#if showIndexing}
      <div class="footer" role="status">Indexing applications…</div>
    {/if}

    {#if largeReady && !largeFull}
      <!-- Window could not be stretched: show the text inside the launcher. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="large inline" role="dialog" aria-label="Large Type" tabindex="-1" onmousedown={(e) => e.preventDefault()} onclick={closeLargeType}>
        <div class="large-text" style:font-size="{largeFont}px">{largeText}</div>
      </div>
    {/if}
  </div>

  {#if largeReady && largeFull}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="large full" role="dialog" aria-label="Large Type" tabindex="-1" onmousedown={(e) => e.preventDefault()} onclick={closeLargeType}>
      <div class="large-text" style:font-size="{largeFont}px">{largeText}</div>
    </div>
  {/if}
</main>

<style>
  .shell {
    padding: 12px;
  }

  .card {
    position: relative;
    isolation: isolate;
    border: 1px solid var(--border);
    border-radius: var(--radius, 14px);
    box-shadow: var(--shadow), var(--card-highlight, inset 0 1px 0 rgba(255, 255, 255, 0.1));
    overflow: hidden;
  }

  .card.opening {
    animation: open 150ms cubic-bezier(0.2, 0.8, 0.25, 1);
  }

  @keyframes open {
    from {
      opacity: 0.4;
      transform: translateY(-6px) scale(0.985);
    }
  }

  /* A blurred backdrop fills the whole window: no padding or drop shadow around the card. */
  :global(:root[data-blur]) .shell {
    padding: 0;
  }

  :global(:root[data-blur]) .card {
    box-shadow: var(--card-highlight, inset 0 1px 0 rgba(255, 255, 255, 0.1));
  }

  @media (prefers-reduced-motion: reduce) {
    .card.opening {
      animation: none;
    }
  }

  /* The background is its own layer so `opacity` fades it, not the content. */
  .card::before {
    content: "";
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    left: 0;
    z-index: -1;
    background: var(--bg);
    opacity: var(--card-opacity, 1);
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    height: max(56px, calc(var(--search-size, calc(22px * var(--font-scale, 1))) * 2.55));
    padding: 0 18px;
  }

  .brand-icon {
    flex: none;
    display: block;
    border-radius: 6px;
  }

  input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--fg);
    caret-color: var(--accent);
    font: inherit;
    font-size: var(--search-size, calc(22px * var(--font-scale, 1)));
    padding: 0;
  }

  input::placeholder {
    color: var(--muted);
    opacity: 0.8;
  }

  .notice {
    padding: 9px 18px 11px;
    border-top: 1px solid var(--border);
    color: var(--warn);
    font-size: calc(12.5px * var(--font-scale, 1));
    line-height: 1.45;
  }

  .results {
    --row-height: var(--row-h, max(48px, calc(48px * var(--font-scale, 1))));
    position: relative;
    max-height: calc(var(--row-height) * 8.5 + 12px);
    padding: 6px;
    overflow-y: auto;
    overscroll-behavior: contain;
    border-top: 1px solid var(--border);
  }

  .results.empty,
  .results.hidden {
    display: none;
  }

  /* A preview pane is open below: fewer rows, so the window stays short. */
  .results.compact {
    max-height: calc(var(--row-height) * 4.5 + 12px);
  }

  /* Result rows are styled in lib/ResultRow.svelte. */

  kbd {
    padding: 1px 6px;
    border: 1px solid var(--kbd-border);
    border-radius: 5px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: calc(11px * var(--font-scale, 1));
    line-height: 1.5;
    color: var(--muted);
  }

  .output {
    max-height: 340px;
    overflow-y: auto;
    padding: 10px 18px 12px;
    border-top: 1px solid var(--border);
    user-select: text;
    -webkit-user-select: text;
  }

  .output-heading {
    margin-bottom: 4px;
    color: var(--muted);
    font-size: calc(12px * var(--font-scale, 1));
    font-weight: 600;
  }

  .output-text {
    margin: 0;
    font: inherit;
    font-size: calc(14px * var(--font-scale, 1));
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .error {
    padding: 9px 18px 11px;
    border-top: 1px solid var(--border);
    color: var(--error);
    font-size: calc(12.5px * var(--font-scale, 1));
    line-height: 1.45;
  }

  .footer {
    padding: 9px 18px 11px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: calc(12px * var(--font-scale, 1));
  }

  /* Modifier shortcuts of the selected row, and the way into the action panel. */
  .hints {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 7px 18px 8px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: calc(11.5px * var(--font-scale, 1));
    white-space: nowrap;
  }

  .chips {
    display: flex;
    gap: 16px;
    min-width: 0;
    overflow: hidden;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }

  .chip.more {
    margin-left: auto;
  }

  .panel {
    padding: 6px;
    border-top: 1px solid var(--border);
  }

  .panel-title {
    padding: 4px 10px 6px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--muted);
    font-size: calc(11.5px * var(--font-scale, 1));
  }

  .action {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    height: max(34px, calc(34px * var(--font-scale, 1)));
    padding: 0 10px;
    border-radius: 8px;
    font-size: calc(14px * var(--font-scale, 1));
  }

  .action.selected {
    background: var(--selected);
  }

  .action .label {
    flex: none;
    max-width: 60%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  /* A preview of what the action produces. */
  .action .detail {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--muted);
    font-size: calc(12px * var(--font-scale, 1));
  }

  .action .keys {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    color: var(--muted);
    font-size: calc(11.5px * var(--font-scale, 1));
  }

  /* Universal Actions can list a couple of dozen entries. */
  .panel.scroll {
    max-height: calc(max(34px, calc(34px * var(--font-scale, 1))) * 8.5 + 40px);
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .panel-title .quote {
    color: var(--fg);
  }

  /* Large Type. `full`: the window covers the screen and only this shows. */
  .card.covered {
    visibility: hidden;
  }

  .large {
    display: grid;
    place-items: center;
    text-align: center;
    cursor: pointer;
  }

  .large.full {
    position: fixed;
    inset: 0;
    padding: 4vmin;
    background: rgba(8, 8, 16, 0.62);
  }

  .large.full .large-text {
    max-width: 94vw;
    max-height: 90vh;
    padding: 4vmin 5vmin;
    border-radius: 3vmin;
    background: var(--bg);
    border: 1px solid var(--border);
    box-shadow: var(--shadow);
  }

  .large.inline {
    min-height: 140px;
    max-height: 520px;
    padding: 16px 24px;
    border-top: 1px solid var(--border);
    overflow: hidden;
  }

  .large-text {
    color: var(--fg);
    font-weight: 600;
    line-height: 1.15;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    overflow: hidden;
  }
</style>
