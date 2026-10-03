<script lang="ts">
  // Grid View: results that are tiles (an emoji, an image, an icon and a short
  // label) laid out in rows. The keys (arrows, Enter, Tab ...) are handled by
  // App.svelte, which asks this component only for how many columns there are.
  import Glyph from "./Glyph.svelte";
  import type { ResultDto } from "./ipc";

  let {
    items,
    selected,
    verb,
    compact = false,
    columns = $bindable(1),
    onhover,
    onrun,
  }: {
    items: ResultDto[];
    selected: number;
    /** What Enter does for the selected tile, as a word. */
    verb: string;
    /** A preview pane is open: show fewer rows. */
    compact?: boolean;
    columns?: number;
    onhover: (index: number) => void;
    onrun: (index: number) => void;
  } = $props();

  let grid: HTMLElement | undefined = $state();
  /** Image URLs that failed to load; those tiles show a built-in glyph instead. */
  let broken = $state<Record<string, boolean>>({});

  const chosen = $derived<ResultDto | undefined>(items[selected]);

  function glyphFor(item: ResultDto): string {
    if (item.icon?.kind === "builtin") return item.icon.name;
    return item.action === "launch" ? "app" : "file";
  }

  // The number of columns is whatever the stylesheet's `auto-fill` produced.
  function measure() {
    if (!grid) return;
    const tracks = getComputedStyle(grid).gridTemplateColumns.split(" ").filter(Boolean).length;
    if (tracks > 0 && tracks !== columns) columns = tracks;
  }

  $effect(() => {
    if (!grid) return;
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(grid);
    return () => observer.disconnect();
  });

  // A new search starts at the top.
  $effect(() => {
    void items;
    if (grid) grid.scrollTop = 0;
  });
</script>

<div class="wrap">
  <div
    id="results"
    class="grid"
    class:compact
    role="listbox"
    aria-label="Results"
    bind:this={grid}
  >
    {#each items as item, i (item.id)}
      <!-- Keyboard handling lives on the window; tiles must not take focus from the input. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        id="result-{i}"
        class="cell"
        class:selected={i === selected}
        role="option"
        aria-selected={i === selected}
        aria-label={item.title}
        tabindex="-1"
        title={item.title}
        onmousemove={(e) => {
          if (e.movementX !== 0 || e.movementY !== 0) onhover(i);
        }}
        onmousedown={(e) => e.preventDefault()}
        onclick={() => onrun(i)}
      >
        <span class="picture">
          {#if item.glyph}
            <span class="emoji" aria-hidden="true">{item.glyph}</span>
          {:else if item.icon?.kind === "url" && !broken[item.icon.url]}
            {@const url = item.icon.url}
            <img src={url} alt="" draggable="false" onerror={() => (broken[url] = true)} />
          {:else}
            <span class="fallback"><Glyph name={glyphFor(item)} /></span>
          {/if}
        </span>
        <span class="label">{item.title}</span>
      </div>
    {/each}
  </div>
  {#if chosen}
    <div class="caption" aria-hidden="true">
      <span class="name">{chosen.title}</span>
      {#if !chosen.glyph && chosen.subtitle}<span class="detail">{chosen.subtitle}</span>{/if}
      <span class="enter"><kbd>↵</kbd>{verb}</span>
    </div>
  {/if}
</div>

<style>
  .wrap {
    border-top: 1px solid var(--border);
  }

  .grid {
    --cell: 76px;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--cell), 1fr));
    gap: 2px;
    max-height: calc((var(--cell) + 2px) * 5 + 12px);
    padding: 6px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .grid.compact {
    max-height: calc((var(--cell) + 2px) * 3 + 12px);
  }

  .cell {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    height: var(--cell);
    padding: 4px 3px;
    border-radius: 10px;
    min-width: 0;
  }

  .cell.selected {
    background: var(--selected);
  }

  .picture {
    display: grid;
    place-items: center;
    width: 40px;
    height: 36px;
    color: var(--muted);
  }

  .emoji {
    font-size: 30px;
    line-height: 1;
    font-family:
      "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", "Twemoji Mozilla", sans-serif;
  }

  .picture img {
    display: block;
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: 4px;
  }

  .fallback {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: var(--tile);
  }

  .label {
    width: 100%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    text-align: center;
    color: var(--muted);
    font-size: calc(10.5px * var(--font-scale, 1));
    line-height: 1.2;
  }

  .cell.selected .label {
    color: var(--fg);
  }

  .caption {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 18px 8px;
    border-top: 1px solid var(--border);
    font-size: calc(12.5px * var(--font-scale, 1));
    white-space: nowrap;
  }

  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .detail {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--muted);
    font-size: calc(11.5px * var(--font-scale, 1));
  }

  .enter {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
    flex: none;
    color: var(--muted);
    font-size: calc(11px * var(--font-scale, 1));
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
