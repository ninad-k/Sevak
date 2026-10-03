<script lang="ts">
  // Text View: one result's long text in a scrollable, full-height view
  // (Ctrl+T on a row that has text; Esc or Left goes back to the list). Scrolling
  // keys are handled by App.svelte, which calls `scrollBy` / `scrollTo`.
  import type { TextViewContent } from "./ipc";

  let { content, mac = false }: { content: TextViewContent | null; mac?: boolean } = $props();

  let body: HTMLElement | undefined = $state();

  /** Scrolls by `delta` pixels, or by a page for `"page"` / `"-page"`. */
  export function scrollBy(delta: number | "page" | "-page") {
    if (!body) return;
    const page = body.clientHeight - 24;
    body.scrollTop += delta === "page" ? page : delta === "-page" ? -page : delta;
  }

  export function scrollToEdge(end: boolean) {
    if (body) body.scrollTop = end ? body.scrollHeight : 0;
  }

  // A different text starts at the top.
  $effect(() => {
    void content;
    if (body) body.scrollTop = 0;
  });

  const mod = $derived(mac ? "⌘" : "Ctrl+");
</script>

<section class="view" aria-label="Text">
  <div class="body" bind:this={body}>
    {#if content}
      <pre>{content.text}</pre>
      {#if content.truncated}<div class="note">The text is longer than what is shown.</div>{/if}
    {:else}
      <div class="note">Loading…</div>
    {/if}
  </div>
  <div class="foot" aria-hidden="true">
    <span class="chip"><kbd>↑</kbd><kbd>↓</kbd>Scroll</span>
    <span class="chip"><kbd>{mod}C</kbd>Copy</span>
    <span class="chip"><kbd>↵</kbd>Run</span>
    <span class="chip more"><kbd>Esc</kbd>/<kbd>←</kbd>Back</span>
  </div>
</section>

<style>
  .view {
    border-top: 1px solid var(--border);
  }

  .body {
    height: 400px;
    padding: 12px 18px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  pre {
    margin: 0;
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: calc(13px * var(--font-scale, 1));
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }

  .note {
    margin-top: 10px;
    color: var(--muted);
    font-size: calc(12px * var(--font-scale, 1));
  }

  .foot {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 7px 18px 8px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: calc(11.5px * var(--font-scale, 1));
    white-space: nowrap;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
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
