<script lang="ts">
  import type { ResultDto } from "./ipc";
  import ResultRow from "./ResultRow.svelte";
  import { applyVariables, previewVariables, SIZE_DEFAULTS, type Mode, type Palette, type ThemeSpec } from "./themes";

  // A live preview of the launcher: the real result row component with mock
  // rows, styled by the same CSS variables the launcher uses. The variables are
  // set on this component's own element, so Settings keeps its own look.
  let {
    spec,
    palette,
    defaults,
    mode,
    query = "chr",
  }: {
    spec: ThemeSpec;
    /** The palette being shown (may lack colors). */
    palette: Palette;
    /** Complete palette of the same mode, for the colors `palette` lacks. */
    defaults: Palette;
    mode: Mode;
    query?: string;
  } = $props();

  const none = { secondary: [], copy_text: null, autocomplete: null };
  const rows: ResultDto[] = [
    { id: "preview:chrome", title: "Google Chrome", subtitle: "Application", icon: { kind: "builtin", name: "app" }, plugin_id: "apps", action: "launch", ...none },
    { id: "preview:calc", title: "Calculator", subtitle: "Application", icon: { kind: "builtin", name: "calculator" }, plugin_id: "apps", action: "launch", ...none },
    { id: "preview:file", title: "quarterly-report-final.xlsx", subtitle: "C:/Users/you/Documents/Reports/2026/Q3", icon: { kind: "builtin", name: "file" }, plugin_id: "files", action: "open_path", ...none },
    { id: "preview:web", title: "Search Google for “chrome themes”", subtitle: "Web search", icon: { kind: "builtin", name: "web" }, plugin_id: "web:g", action: "open_url", ...none },
  ];
  const verbs = ["Launch", "Launch", "Open", "Search"];
  const glyphs = ["app", "calculator", "file", "web"];

  let host: HTMLElement | undefined = $state();
  let applied: string[] = [];

  $effect(() => {
    if (host) applied = applyVariables(host, previewVariables(spec, palette, defaults, mode), applied);
  });

  const width = $derived(spec.layout.window_width ?? SIZE_DEFAULTS.window_width);
</script>

<div class="stage" bind:this={host} aria-hidden="true" inert>
  <div class="card" style:max-width="{width}px">
    <div class="bar">
      <img class="brand-icon" src="/sevak-icon.png" width="28" height="28" alt="" draggable="false" />
      <input type="text" readonly tabindex="-1" value={query} placeholder="Type to search…" />
    </div>
    <div class="results">
      {#each rows as item, i (item.id)}
        <ResultRow id="preview-{i}" {item} index={i} selected={i === 0} glyph={glyphs[i]} verb={verbs[i]} />
      {/each}
    </div>
    <div class="hints">
      <span class="chips"><span class="chip"><kbd>Ctrl</kbd>Show in folder</span></span>
      <span class="chip more"><kbd>→</kbd>Actions</span>
    </div>
  </div>
</div>

<style>
  /* A stand-in desktop, so a transparent background is visible as such. */
  .stage {
    display: grid;
    justify-items: center;
    padding: 22px 18px;
    border-radius: 12px;
    border: 1px solid var(--border);
    background:
      linear-gradient(135deg, #3a6ea5 0%, #7b5ea7 45%, #d9795b 100%);
    color: var(--fg);
    font-family: inherit;
  }

  /* The rules below follow App.svelte's .card, .bar, .results and .hints. */
  .card {
    position: relative;
    isolation: isolate;
    width: 100%;
    border: 1px solid var(--border);
    border-radius: var(--radius, 14px);
    box-shadow: var(--shadow);
    overflow: hidden;
    font-size: var(--font-size, 15px);
  }

  .card::before {
    content: "";
    position: absolute;
    inset: 0;
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

  .results {
    --row-height: var(--row-h, max(48px, calc(48px * var(--font-scale, 1))));
    padding: 6px;
    border-top: 1px solid var(--border);
  }

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
</style>
