<script lang="ts">
  // The file buffer above the results: a strip of chips for the files and
  // folders collected so far, and below it the single line that says what the
  // buffer is doing right now (picking a destination, a running action, how the
  // last one ended). All state lives in App.svelte and the shell; this only draws.
  import { tick } from "svelte";
  import Glyph from "./Glyph.svelte";
  import type { BufferItemDto, BufferNote, BufferProgress } from "./ipc";

  interface Props {
    items: BufferItemDto[];
    /** A slow action is running. */
    progress?: BufferProgress | null;
    /** How the last action ended. */
    note?: BufferNote | null;
    /** Move to… / Copy to… is waiting for a folder. */
    destination?: { label: string; count: number } | null;
    /** Draws Alt as ⌥. */
    mac?: boolean;
    onremove: (index: number) => void;
  }

  let { items, progress = null, note = null, destination = null, mac = false, onremove }: Props =
    $props();

  let list: HTMLElement | undefined = $state();
  /** Icon URLs that failed to load; those chips show a built-in glyph instead. */
  let brokenIcons = $state<Record<string, boolean>>({});

  const alt = $derived(mac ? "⌥" : "Alt+");
  const ctrl = $derived(mac ? "⌘" : "Ctrl+");
  const percent = $derived(
    progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0,
  );

  // The newest chip stays in view.
  $effect(() => {
    void items.length;
    void tick().then(() => {
      if (list) list.scrollLeft = list.scrollWidth;
    });
  });
</script>

{#if items.length > 0 || progress || note || destination}
  <section class="buffer" aria-label="File buffer">
    {#if items.length > 0}
      <div class="strip">
        <span class="label">Buffer</span>
        <ul class="chips" bind:this={list}>
          {#each items as item, i (item.path)}
            <li class="chip" title={item.path}>
              <span class="icon">
                {#if item.icon?.kind === "url" && !brokenIcons[item.icon.url]}
                  {@const url = item.icon.url}
                  <img
                    src={url}
                    width="16"
                    height="16"
                    alt=""
                    draggable="false"
                    onerror={() => (brokenIcons[url] = true)}
                  />
                {:else}
                  <Glyph name={item.icon?.kind === "builtin" ? item.icon.name : item.is_dir ? "folder" : "file"} />
                {/if}
              </span>
              <span class="name">{item.name}</span>
              <button
                class="remove"
                type="button"
                tabindex="-1"
                aria-label="Remove {item.name} from the buffer"
                onmousedown={(e) => e.preventDefault()}
                onclick={() => onremove(i)}>×</button
              >
            </li>
          {/each}
        </ul>
        <span class="keys" aria-hidden="true">
          <span class="key"><kbd>{alt}→</kbd>Actions</span>
          <span class="key"><kbd>{alt}⌫</kbd>Clear</span>
        </span>
      </div>
    {/if}

    {#if destination}
      <div class="line" role="status">
        <strong>{destination.label}</strong>
        <span class="muted">
          {destination.count === 1 ? "1 item" : `${destination.count} items`}: type or browse to a
          folder.
        </span>
        <span class="keys" aria-hidden="true">
          <span class="key"><kbd>↵</kbd>Use folder</span>
          <span class="key"><kbd>{ctrl}↵</kbd>Use typed path</span>
          <span class="key"><kbd>Tab</kbd>Open</span>
          <span class="key"><kbd>Esc</kbd>Cancel</span>
        </span>
      </div>
    {/if}

    {#if progress}
      <div class="line" role="status">
        <span class="progress-text">
          {progress.label}{progress.total > 0
            ? ` · ${Math.min(progress.done + 1, progress.total)} of ${progress.total}`
            : ""}{progress.name ? ` · ${progress.name}` : "…"}
        </span>
        <span
          class="bar"
          role="progressbar"
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={percent}
        >
          <span class="fill" style:width="{percent}%"></span>
        </span>
      </div>
    {:else if note}
      <div
        class="line"
        class:ok={!note.error}
        class:error={note.error}
        role={note.error ? "alert" : "status"}
      >
        {note.text}
      </div>
    {/if}
  </section>
{/if}

<style>
  .buffer {
    border-top: 1px solid var(--border);
    font-size: calc(12px * var(--font-scale, 1));
    color: var(--muted);
  }

  .strip,
  .line {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 18px;
    min-width: 0;
  }

  .strip + .line,
  .line + .line {
    border-top: 1px solid var(--border);
  }

  .label {
    flex: none;
    font-size: calc(10.5px * var(--font-scale, 1));
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .chips {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
    overflow-x: auto;
    scroll-behavior: smooth;
  }

  .chip {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 180px;
    height: calc(22px * var(--font-scale, 1));
    padding: 0 4px 0 6px;
    border-radius: 11px;
    background: var(--selected);
    color: var(--fg);
  }

  .icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    color: var(--muted);
  }

  .icon :global(svg) {
    width: 14px;
    height: 14px;
  }

  .icon img {
    display: block;
    width: 16px;
    height: 16px;
    object-fit: contain;
  }

  .name {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .remove {
    flex: none;
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
  }

  .remove:hover {
    background: var(--tile);
    color: var(--fg);
  }

  .keys {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    margin-left: auto;
    white-space: nowrap;
  }

  .key {
    display: inline-flex;
    align-items: center;
    gap: 6px;
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

  .line strong {
    flex: none;
    color: var(--fg);
    font-weight: 600;
  }

  .muted {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .progress-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--fg);
  }

  .bar {
    flex: none;
    width: 120px;
    height: 4px;
    border-radius: 2px;
    background: var(--tile);
    overflow: hidden;
  }

  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.2s ease;
  }

  .line.ok {
    color: var(--ok);
  }

  .line.error {
    color: var(--error);
  }
</style>
