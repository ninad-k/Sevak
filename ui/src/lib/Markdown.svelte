<script lang="ts">
  // Rendered Markdown. The HTML comes from `renderMarkdown`, which escapes all
  // text and only emits a fixed set of elements (see markdown.ts), so inserting
  // it is safe. Links never navigate the webview: a click is intercepted and
  // handed to the shell, which opens http, https and mailto addresses in the
  // default browser.
  import { t } from "./i18n.svelte";
  import { openLink } from "./ipc";
  import { renderMarkdown } from "./markdown";

  let { source, compact = false }: { source: string; compact?: boolean } = $props();

  const html = $derived(
    renderMarkdown(source, {
      alertLabels: {
        note: t("markdown.note"),
        tip: t("markdown.tip"),
        important: t("markdown.important"),
        warning: t("markdown.warning"),
        caution: t("markdown.caution"),
      },
    }),
  );

  function onLink(event: MouseEvent) {
    const link = (event.target as Element | null)?.closest?.("a[href]");
    if (!link) return;
    event.preventDefault();
    void openLink(link.getAttribute("href") ?? "");
  }
</script>

<!-- Keyboard activation of a link is a click event too. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="md" class:compact onclick={onLink} onauxclick={onLink}>
  {@html html}
</div>

<style>
  .md {
    --md-note: var(--accent-strong);
    --md-tip: var(--ok);
    --md-important: #8b5cf6; /* readable on light and dark surfaces */
    --md-warning: var(--warn);
    --md-caution: var(--error);
    font-size: calc(13.5px * var(--font-scale, 1));
    line-height: 1.55;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }

  .md > :global(:first-child) {
    margin-top: 0;
  }

  .md > :global(:last-child) {
    margin-bottom: 0;
  }

  .md :global(p),
  .md :global(ul),
  .md :global(ol),
  .md :global(blockquote),
  .md :global(pre),
  .md :global(.md-table-wrap),
  .md :global(.md-alert) {
    margin: 0 0 0.7em;
  }

  /* Long headings are cut with an ellipsis instead of wrapping. */
  .md :global(.md-h) {
    margin: 0.9em 0 0.4em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 1.3;
    font-weight: 650;
  }

  .md :global(h1) {
    font-size: 1.5em;
    padding-bottom: 0.2em;
    border-bottom: 1px solid var(--border);
  }

  .md :global(h2) {
    font-size: 1.3em;
    padding-bottom: 0.15em;
    border-bottom: 1px solid var(--border);
  }

  .md :global(h3) {
    font-size: 1.15em;
  }

  .md :global(h4),
  .md :global(h5),
  .md :global(h6) {
    font-size: 1em;
  }

  .md :global(h5),
  .md :global(h6) {
    color: var(--muted);
  }

  .md :global(a) {
    color: var(--accent-strong);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }

  .md :global(a:hover) {
    text-decoration-thickness: 2px;
  }

  .md :global(a:focus-visible) {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
    border-radius: 2px;
  }

  .md :global(code) {
    padding: 0.1em 0.35em;
    border-radius: 4px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 0.9em;
  }

  .md :global(pre.md-code) {
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--kbd-bg);
    overflow-x: auto;
    line-height: 1.45;
  }

  .md :global(pre.md-code code) {
    padding: 0;
    background: none;
    font-size: 0.88em;
    white-space: pre;
  }

  .md :global(blockquote) {
    padding: 0 0 0 12px;
    border-left: 3px solid var(--border);
    color: var(--muted);
  }

  .md :global(ul),
  .md :global(ol) {
    padding-left: 1.6em;
  }

  .md :global(li > ul),
  .md :global(li > ol) {
    margin: 0.2em 0 0;
  }

  .md :global(li + li) {
    margin-top: 0.15em;
  }

  .md :global(ul.md-tasks) {
    list-style: none;
    padding-left: 0.4em;
  }

  .md :global(.md-task) {
    margin: 0 0.4em 0 0;
    vertical-align: -1px;
    accent-color: var(--accent-strong);
  }

  .md :global(hr) {
    margin: 1em 0;
    border: 0;
    border-top: 1px solid var(--border);
  }

  .md :global(.md-img-alt) {
    color: var(--muted);
    font-style: italic;
  }

  .md :global(.md-img) {
    max-width: 100%;
    height: auto;
    border-radius: 4px;
  }

  /* Tables: widths come from the content (<col width>), the layout is fixed. */
  .md :global(.md-table-wrap) {
    overflow-x: auto;
  }

  .md :global(table.md-table) {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
  }

  .md :global(.md-table th),
  .md :global(.md-table td) {
    padding: 4px 10px;
    border: 1px solid var(--border);
    vertical-align: top;
    text-align: left;
    overflow-wrap: anywhere;
  }

  .md :global(.md-table th) {
    background: var(--kbd-bg);
    font-weight: 600;
  }

  .md :global(.md-table .md-center) {
    text-align: center;
  }

  .md :global(.md-table .md-right) {
    text-align: right;
  }

  /* GitHub alerts: an accent bar and a title in the alert's colour. */
  .md :global(.md-alert) {
    --alert: var(--md-note);
    padding: 6px 12px 7px;
    border-left: 3px solid var(--alert);
    border-radius: 0 6px 6px 0;
    background: color-mix(in srgb, var(--alert) 9%, transparent);
  }

  .md :global(.md-alert-tip) {
    --alert: var(--md-tip);
  }

  .md :global(.md-alert-important) {
    --alert: var(--md-important);
  }

  .md :global(.md-alert-warning) {
    --alert: var(--md-warning);
  }

  .md :global(.md-alert-caution) {
    --alert: var(--md-caution);
  }

  .md :global(.md-alert-title) {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-bottom: 3px;
    color: var(--alert);
    font-weight: 600;
  }

  .md :global(.md-alert-icon) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.35em;
    height: 1.35em;
    border: 1.5px solid currentColor;
    border-radius: 50%;
    font-size: 0.78em;
    line-height: 1;
  }

  .md :global(.md-alert > :last-child) {
    margin-bottom: 0;
  }

  .md.compact {
    font-size: calc(12.5px * var(--font-scale, 1));
    line-height: 1.45;
  }
</style>
