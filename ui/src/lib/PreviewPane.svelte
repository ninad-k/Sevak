<script lang="ts">
  // The preview pane (Shift / Ctrl+Y): what the selected row is, below the
  // list. Everything shown was prepared by the shell (see `preview` in ipc.ts);
  // nothing here reads files or the network.
  import Glyph from "./Glyph.svelte";
  import type { PreviewContent } from "./ipc";

  let {
    content,
    loading,
    glyph = null,
    hasTextView = false,
    mac = false,
  }: {
    content: PreviewContent | null;
    loading: boolean;
    /** A tile's text picture (an emoji), drawn large. */
    glyph?: string | null;
    /** Ctrl+T opens the Text View for this row. */
    hasTextView?: boolean;
    mac?: boolean;
  } = $props();

  /** Pixel size of the image shown, once it has loaded. */
  let imageSize = $state<string | null>(null);

  const modified = $derived(
    content?.modified != null ? new Date(content.modified * 1000).toLocaleString() : null,
  );
  const mod = $derived(mac ? "⌘" : "Ctrl+");

  $effect(() => {
    // A different picture: forget the size of the last one.
    void (content?.body.kind === "image" ? content.body.src : null);
    imageSize = null;
  });
</script>

<section class="pane" class:loading aria-label="Preview">
  {#if glyph}
    <div class="big" aria-hidden="true">{glyph}</div>
  {/if}

  {#if content}
    {#if content.body.kind === "text"}
      <pre class="text">{content.body.text}</pre>
      {#if content.body.truncated}
        <div class="note">Showing the start only.</div>
      {/if}
    {:else if content.body.kind === "image"}
      <div class="picture">
        <img
          src={content.body.src}
          alt=""
          draggable="false"
          onload={(e) => {
            const img = e.currentTarget as HTMLImageElement;
            imageSize = `${img.naturalWidth} × ${img.naturalHeight} px`;
          }}
        />
      </div>
    {:else if content.body.kind === "folder"}
      <ul class="folder">
        {#each content.body.entries as entry (entry.name)}
          <li class:dir={entry.dir}>
            <Glyph name={entry.dir ? "folder" : "file"} />
            <span>{entry.name}</span>
          </li>
        {/each}
        {#if content.body.entries.length === 0}
          <li class="empty">This folder is empty.</li>
        {/if}
      </ul>
      {#if content.body.truncated}
        <div class="note">Showing the first {content.body.entries.length} entries.</div>
      {/if}
    {:else if content.body.kind === "url"}
      <div class="url">{content.body.url}</div>
    {/if}

    {#if content.note}
      <div class="note">{content.note}</div>
    {/if}

    {#if content.meta.length > 0 || modified || imageSize}
      <dl class="meta">
        {#each content.meta as row (row.label)}
          <dt>{row.label}</dt>
          <dd title={row.value}>{row.value}</dd>
        {/each}
        {#if imageSize}
          <dt>Dimensions</dt>
          <dd>{imageSize}</dd>
        {/if}
        {#if modified}
          <dt>Modified</dt>
          <dd>{modified}</dd>
        {/if}
      </dl>
    {/if}
  {:else if !glyph}
    <div class="note">{loading ? "Loading…" : "Nothing to preview."}</div>
  {/if}

  <div class="foot" aria-hidden="true">
    {#if hasTextView}
      <span class="chip"><kbd>{mod}T</kbd>Full text</span>
    {/if}
    <span class="chip more"><kbd>Shift</kbd>/<kbd>{mod}Y</kbd>Close</span>
  </div>
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: 310px;
    padding: 10px 18px 0;
    border-top: 1px solid var(--border);
    overflow: hidden;
    transition: opacity 80ms;
  }

  .pane.loading {
    opacity: 0.7;
  }

  .big {
    text-align: center;
    font-size: 64px;
    line-height: 1.1;
    font-family:
      "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", "Twemoji Mozilla", sans-serif;
  }

  .text {
    flex: 0 1 auto;
    min-height: 0;
    max-height: 140px;
    margin: 0;
    padding: 8px 10px;
    overflow: auto;
    overscroll-behavior: contain;
    border-radius: 8px;
    background: var(--surface);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: calc(12px * var(--font-scale, 1));
    line-height: 1.45;
    white-space: pre;
    user-select: text;
    -webkit-user-select: text;
  }

  .picture {
    flex: none;
    display: grid;
    place-items: center;
    padding: 4px;
    border-radius: 8px;
    background: var(--surface);
  }

  .picture img {
    display: block;
    max-width: 100%;
    max-height: 140px;
    object-fit: contain;
  }

  .folder {
    flex: none;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 1px 14px;
    max-height: 140px;
    margin: 0;
    padding: 6px 8px;
    overflow: auto;
    overscroll-behavior: contain;
    list-style: none;
    border-radius: 8px;
    background: var(--surface);
    font-size: calc(12.5px * var(--font-scale, 1));
  }

  .folder li {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    height: 22px;
  }

  .folder li :global(svg) {
    flex: none;
    width: 14px;
    height: 14px;
    color: var(--muted);
  }

  .folder li.dir span {
    font-weight: 600;
  }

  .folder li span {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .folder .empty {
    grid-column: 1 / -1;
    color: var(--muted);
  }

  .url {
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--surface);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: calc(12px * var(--font-scale, 1));
    line-height: 1.45;
    overflow-wrap: anywhere;
    max-height: 100px;
    overflow: hidden;
    user-select: text;
    -webkit-user-select: text;
  }

  .note {
    color: var(--muted);
    font-size: calc(12px * var(--font-scale, 1));
  }

  .meta {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 2px 14px;
    margin: 0;
    font-size: calc(12px * var(--font-scale, 1));
  }

  .meta dt {
    color: var(--muted);
  }

  .meta dd {
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .foot {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 4px 0 8px;
    color: var(--muted);
    font-size: calc(11.5px * var(--font-scale, 1));
    white-space: nowrap;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
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
