<script lang="ts">
  import Glyph from "./Glyph.svelte";
  import type { ResultDto } from "./ipc";

  // One row of the result list. The launcher (App.svelte) and the theme editor's
  // preview both render this component, so the preview is the real thing.
  let {
    id,
    item,
    index,
    selected,
    glyph,
    verb,
    broken = false,
    onbroken,
    onmove,
    onrun,
  }: {
    /** DOM id, which the search box points `aria-activedescendant` at. */
    id: string;
    item: ResultDto;
    index: number;
    selected: boolean;
    /** The built-in glyph shown when the row has no (working) icon. */
    glyph: string;
    /** What Enter does with this row, in a word. */
    verb: string;
    /** The icon URL failed to load; show the glyph instead. */
    broken?: boolean;
    onbroken?: () => void;
    onmove?: (event: MouseEvent) => void;
    onrun?: () => void;
  } = $props();
</script>

<!-- Keyboard handling lives on the window; rows must not take focus from the input. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  {id}
  class="row"
  class:selected
  role="option"
  aria-selected={selected}
  tabindex="-1"
  title={item.id}
  onmousemove={onmove}
  onmousedown={(e) => e.preventDefault()}
  onclick={onrun}
>
  <span class="tile">
    {#if item.icon?.kind === "url" && !broken}
      <img
        src={item.icon.url}
        width="32"
        height="32"
        alt=""
        draggable="false"
        onerror={onbroken}
      />
    {:else}
      <Glyph name={glyph} />
    {/if}
  </span>
  <span class="text">
    <span class="title">{item.title}</span>
    {#if item.subtitle}<span class="subtitle">{item.subtitle}</span>{/if}
  </span>
  <span class="hint" aria-hidden="true">
    {#if selected}
      <kbd>↵</kbd><span class="verb">{verb}</span>
    {:else if index < 9}
      <kbd>Ctrl+{index + 1}</kbd>
    {/if}
  </span>
</div>

<style>
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

  /* An unset --selected-fg keeps the title in the normal text color. */
  .row.selected .title {
    color: var(--selected-fg, var(--fg));
  }

  .tile {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--icon-size, 32px);
    height: var(--icon-size, 32px);
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
    width: var(--icon-size, 32px);
    height: var(--icon-size, 32px);
    object-fit: contain;
  }

  /* The built-in glyphs are 20 px at the default icon size. */
  .tile :global(svg) {
    width: calc(var(--icon-size, 32px) * 0.625);
    height: calc(var(--icon-size, 32px) * 0.625);
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
    font-size: var(--font-size, 15px);
    line-height: 1.3;
  }

  .subtitle {
    font-size: calc(12px * var(--font-scale, 1));
    line-height: 1.3;
    color: var(--muted);
  }

  .hint {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: calc(11px * var(--font-scale, 1));
    color: var(--muted);
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
