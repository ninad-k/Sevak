<script lang="ts">
  import { onMount, tick } from "svelte";
  import Glyph from "./lib/Glyph.svelte";
  import { parentPath } from "./lib/path";
  import { applyTheme } from "./lib/theme";
  import {
    execute,
    getStatus,
    hideWindow,
    onHidden,
    onIndex,
    onShow,
    onStatus,
    queryHistory,
    search,
    setContentHeight,
    type ResultDto,
    type Status,
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
  let list: HTMLElement | undefined = $state();

  const hotkeyError = $derived(status?.hotkey.error ?? null);
  const accelerator = $derived(status?.hotkey.accelerator ?? "");
  const hasQuery = $derived(query.trim() !== "");
  // While the index is still filling, a query often yields nothing but the
  // "search the web" fallback; say why instead of looking like a miss.
  const showIndexing = $derived(
    indexing &&
      hasQuery &&
      !error &&
      results.every((result) => result.plugin_id.startsWith("web:")),
  );

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
    if (!shell) return;
    const h = Math.ceil(shell.offsetHeight);
    if (h <= 0 || h === lastHeight) return;
    lastHeight = h;
    void setContentHeight(h);
  }

  function reset() {
    searchSeq++; // drop any search still in flight
    historyPos = -1;
    query = "";
    results = [];
    selected = 0;
    error = null;
    brokenIcons = {};
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
    results = found.results;
    resultsTicket = found.ticket;
    selected = 0;
    if (list) list.scrollTop = 0;
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

  async function run(index: number) {
    const item = results[index];
    if (!item || executing) return;
    executing = true;
    try {
      const message = await execute(item.id, resultsTicket);
      if (message !== null) error = message;
    } finally {
      executing = false;
    }
  }

  /** Moves the selection; `reveal` scrolls it into view (keyboard only). */
  async function select(index: number, reveal = false) {
    if (results.length === 0) return;
    selected = index;
    if (!reveal) return;
    await tick();
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

  function onKeydown(e: KeyboardEvent) {
    if (e.isComposing) return;
    const key = e.key;
    const ctrl = (e.ctrlKey || e.metaKey) && !e.altKey;
    const lower = key.toLowerCase();

    if (key === "Escape") {
      e.preventDefault();
      void hideWindow();
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
      move(1);
    } else if (key === "ArrowUp" || (ctrl && lower === "p")) {
      e.preventDefault();
      move(-1);
    } else if (key === "PageDown") {
      e.preventDefault();
      page(7);
    } else if (key === "PageUp") {
      e.preventDefault();
      page(-7);
    } else if (key === "Enter") {
      e.preventDefault();
      void run(selected);
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
    if (item.plugin_id.startsWith("web:")) return "Search";
    switch (item.action) {
      case "launch":
        return "Launch";
      case "copy_text":
        return "Copy";
      case "custom":
        return "Run";
      default:
        return "Open";
    }
  }

  /** The glyph for rows without a (working) image. */
  function glyphFor(item: ResultDto): string {
    if (item.icon?.kind === "builtin") return item.icon.name;
    return item.action === "launch" ? "app" : "file";
  }

  onMount(() => {
    focusInput(true);

    const resizeObserver = new ResizeObserver(reportHeight);
    if (shell) resizeObserver.observe(shell);
    reportHeight();

    getStatus().then((s) => {
      if (s) {
        status = s;
        indexing = s.indexing;
        applyTheme(s.theme);
      }
    });

    const unlisteners = [
      onShow(() => {
        reset();
        focusInput(true);
      }),
      onHidden(reset),
      onStatus((s) => {
        status = s;
        indexing = s.indexing;
        applyTheme(s.theme);
      }),
      onIndex((state) => {
        indexing = state === "indexing";
      }),
    ];

    return () => {
      resizeObserver.disconnect();
      for (const u of unlisteners) void u.then((fn) => fn());
    };
  });
</script>

<svelte:window onkeydown={onKeydown} onfocus={() => focusInput()} oncontextmenu={onContextMenu} />

<main class="shell" bind:this={shell}>
  <div class="card">
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
        Shortcut "{accelerator}" is unavailable: {hotkeyError}. Choose another one in Settings
        (tray menu).
      </div>
    {/if}

    <div
      id="results"
      class="results"
      class:empty={results.length === 0}
      role="listbox"
      aria-label="Results"
      bind:this={list}
    >
      {#each results as item, i (item.id)}
        <!-- Keyboard handling lives on the window; rows must not take focus from the input. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          id="result-{i}"
          class="row"
          class:selected={i === selected}
          role="option"
          aria-selected={i === selected}
          tabindex="-1"
          onmousemove={(e) => onRowMove(e, i)}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => void run(i)}
        >
          <span class="tile">
            {#if item.icon?.kind === "url" && !brokenIcons[item.icon.url]}
              {@const url = item.icon.url}
              <img
                src={url}
                width="32"
                height="32"
                alt=""
                draggable="false"
                onerror={() => (brokenIcons[url] = true)}
              />
            {:else}
              <Glyph name={glyphFor(item)} />
            {/if}
          </span>
          <span class="text">
            <span class="title">{item.title}</span>
            {#if item.subtitle}<span class="subtitle">{item.subtitle}</span>{/if}
          </span>
          <span class="hint" aria-hidden="true">
            {#if i === selected}
              <kbd>↵</kbd><span class="verb">{verb(item)}</span>
            {:else if i < 9}
              <kbd>Ctrl+{i + 1}</kbd>
            {/if}
          </span>
        </div>
      {/each}
    </div>

    {#if error}
      <div class="error" role="alert">{error}</div>
    {/if}
    {#if showIndexing}
      <div class="footer" role="status">Indexing applications…</div>
    {/if}
  </div>
</main>

<style>
  .shell {
    padding: 12px;
  }

  .card {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 56px;
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
    font-size: 22px;
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
    font-size: 12.5px;
    line-height: 1.45;
  }

  .results {
    --row-height: 48px;
    position: relative;
    max-height: calc(var(--row-height) * 8.5 + 12px);
    padding: 6px;
    overflow-y: auto;
    overscroll-behavior: contain;
    border-top: 1px solid var(--border);
  }

  .results.empty {
    display: none;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    height: var(--row-height);
    padding: 0 10px;
    border-radius: 10px;
  }

  .row.selected {
    background: var(--selected);
  }

  .tile {
    flex: none;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: var(--tile);
    color: var(--muted);
    overflow: hidden;
  }

  .tile:has(img) {
    background: transparent;
    border-radius: 0;
  }

  .tile img {
    display: block;
    width: 32px;
    height: 32px;
    object-fit: contain;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .title,
  .subtitle {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .title {
    font-size: 15px;
    line-height: 1.3;
  }

  .subtitle {
    font-size: 12px;
    line-height: 1.3;
    color: var(--muted);
  }

  .hint {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--muted);
  }

  kbd {
    padding: 1px 6px;
    border: 1px solid var(--kbd-border);
    border-radius: 5px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 11px;
    line-height: 1.5;
    color: var(--muted);
  }

  .error {
    padding: 9px 18px 11px;
    border-top: 1px solid var(--border);
    color: var(--error);
    font-size: 12.5px;
    line-height: 1.45;
  }

  .footer {
    padding: 9px 18px 11px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: 12px;
  }
</style>
