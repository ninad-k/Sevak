<script lang="ts">
  // "View in Sevak": one selected file (a picture, a PDF's first page, text or
  // Markdown) shown full height inside the launcher. With several files the
  // arrow keys (handled by App.svelte, which calls `scrollBy` / `scrollToEdge`)
  // step through them. The content was prepared by the shell like the preview
  // pane's; nothing here reads files.
  import Markdown from "./Markdown.svelte";
  import { t } from "./i18n.svelte";
  import type { PreviewContent } from "./ipc";

  let { content, index, count }: { content: PreviewContent | null; index: number; count: number } =
    $props();

  let body: HTMLElement | undefined = $state();

  export function scrollBy(delta: number | "page" | "-page") {
    if (!body) return;
    const page = body.clientHeight - 24;
    body.scrollTop += delta === "page" ? page : delta === "-page" ? -page : delta;
  }

  export function scrollToEdge(end: boolean) {
    if (body) body.scrollTop = end ? body.scrollHeight : 0;
  }

  // A different file starts at the top.
  $effect(() => {
    void content;
    if (body) body.scrollTop = 0;
  });

  const several = $derived(count > 1);
</script>

<section class="view" aria-label={t("launcher.fileView.label")}>
  <div class="head">
    <span class="name" title={content?.title}>{content?.title ?? ""}</span>
    {#if several}<span class="count">{index + 1} / {count}</span>{/if}
  </div>
  <div class="body" bind:this={body} aria-live="polite">
    {#if !content}
      <div class="note">{t("launcher.textView.loading")}</div>
    {:else if content.body.kind === "image"}
      <div class="picture"><img src={content.body.src} alt={content.title} draggable="false" /></div>
    {:else if content.body.kind === "markdown"}
      <Markdown source={content.body.text} />
    {:else if content.body.kind === "text"}
      <pre>{content.body.text}</pre>
    {/if}
    {#if content && (content.body.kind === "text" || content.body.kind === "markdown") && content.body.truncated}
      <div class="note">{t("launcher.preview.startOnly")}</div>
    {/if}
    {#if content?.note}<div class="note">{content.note}</div>{/if}
  </div>
  <div class="foot" aria-hidden="true">
    {#if several}
      <span class="chip"><kbd>←</kbd><kbd>→</kbd>{t("launcher.fileView.step")}</span>
    {/if}
    <span class="chip"><kbd>↑</kbd><kbd>↓</kbd>{t("launcher.textView.scroll")}</span>
    <span class="chip more"><kbd>Esc</kbd>{#if !several}/<kbd>←</kbd>{/if}{t("launcher.textView.back")}</span>
  </div>
</section>

<style>
  .view {
    border-top: 1px solid var(--border);
  }

  .head {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding: 8px 18px 0;
    font-size: calc(12.5px * var(--font-scale, 1));
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }

  .count {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .body {
    height: 400px;
    padding: 10px 18px 12px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .picture {
    display: grid;
    place-items: center;
    min-height: 100%;
  }

  .picture img {
    display: block;
    max-width: 100%;
    max-height: 376px;
    object-fit: contain;
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
