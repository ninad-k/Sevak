<script lang="ts">
  // Settings > Help: a diagnostics report for bug reports. It is made on this
  // computer, shown here exactly as it will be copied or saved, and never sent
  // anywhere.
  import { onMount } from "svelte";
  import { copyDiagnostics, getDiagnostics, saveDiagnostics } from "./diagnostics-ipc";
  import { openLogDir } from "./settings-ipc";
  import { t, tHtml, type MessageKey } from "./i18n.svelte";

  let report = $state("");
  let loading = $state(true);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  // The file search filters (crates/sevak-plugins/src/files/filters.rs). The
  // examples are query syntax and stay as typed; only their meaning is translated.
  const FILTERS: [string, MessageKey][] = [
    ["ext:pdf  ext:jpg,png", "help.filters.ext"],
    ["type:image", "help.filters.type"],
    ["size:>10mb  size:<500kb", "help.filters.size"],
    ["modified:today  modified:7d", "help.filters.modified"],
    ["modified:>2026-01-01", "help.filters.modifiedAfter"],
    ["created:yesterday", "help.filters.created"],
    ["in:~/Projects  in:src", "help.filters.in"],
    ["is:folder  is:file", "help.filters.is"],
    ['"annual report"', "help.filters.phrase"],
    ["-ext:tmp  -draft", "help.filters.exclude"],
  ];

  async function make() {
    loading = true;
    error = null;
    notice = null;
    const result = await getDiagnostics();
    loading = false;
    if (result.ok) report = result.value;
    else error = t("help.makeFailed", { error: result.error });
  }

  function say(text: string) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 4000);
  }

  async function copy() {
    const result = await copyDiagnostics(report);
    if (result.ok) say(t("help.copied"));
    else error = t("help.copyFailed", { error: result.error });
  }

  async function save() {
    const result = await saveDiagnostics(report);
    if (!result.ok) error = result.error;
    else if (result.value) say(t("help.savedTo", { path: result.value }));
  }

  async function logs() {
    const failure = await openLogDir();
    if (failure) error = failure;
  }

  onMount(() => {
    void make();
    return () => clearTimeout(noticeTimer);
  });
</script>

<div class="hp-page">
  <h1>{t("help.title")}</h1>
  <p class="hp-lead">{@html tHtml("help.lead")}</p>

  <div class="hp-toolbar">
    <button type="button" class="hp-btn primary" disabled={loading || !report} onclick={copy}>
      {t("help.copy")}
    </button>
    <button type="button" class="hp-btn" disabled={loading || !report} onclick={save}>
      {t("help.saveAs")}
    </button>
    <button type="button" class="hp-btn" onclick={logs}>{t("help.openLogs")}</button>
    <button type="button" class="hp-btn" disabled={loading} onclick={make}>
      {loading ? t("help.making") : t("help.refresh")}
    </button>
  </div>

  {#if error}
    <p class="hp-msg error" role="alert">{error}</p>
  {:else if notice}
    <p class="hp-msg ok" role="status">{notice}</p>
  {/if}

  <label class="hp-label" for="hp-report">{t("help.reportLabel")}</label>
  <textarea
    id="hp-report"
    class="hp-report"
    readonly
    spellcheck="false"
    rows="18"
    value={loading && !report ? t("help.makingReport") : report}
  ></textarea>

  <p class="hp-hint">{@html tHtml("help.cliHint")}</p>

  <section class="hp-cheats" aria-labelledby="hp-filters">
    <h2 id="hp-filters">{t("help.filters.heading")}</h2>
    <p class="hp-hint">{@html tHtml("help.filters.hint")}</p>
    <table class="hp-table">
      <thead class="sr-only">
        <tr><th scope="col">{t("help.filters.filter")}</th><th scope="col">{t("help.filters.meaning")}</th></tr>
      </thead>
      <tbody>
        {#each FILTERS as [example, meaning] (example)}
          <tr>
            <td><code>{example}</code></td>
            <td>{t(meaning)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
</div>

<style>
  .hp-page h1 {
    margin: 0 0 6px;
    font-size: 19px;
    font-weight: 650;
  }

  .hp-lead {
    margin: 0 0 14px;
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.5;
  }

  .hp-toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 10px;
  }

  .hp-btn {
    height: 30px;
    padding: 0 14px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .hp-btn:hover:not(:disabled) {
    border-color: var(--accent-strong);
  }

  .hp-btn:focus-visible,
  .hp-report:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .hp-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .hp-btn.primary {
    border-color: var(--accent-strong);
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
  }

  .hp-btn.primary:hover:not(:disabled) {
    background: var(--accent-strong);
  }

  .hp-msg {
    margin: 0 0 8px;
    font-size: 12.5px;
  }

  .hp-msg.ok {
    color: var(--ok);
  }

  .hp-msg.error {
    color: var(--error);
  }

  .hp-label {
    display: block;
    margin-bottom: 4px;
    font-size: 12px;
    font-weight: 600;
  }

  .hp-report {
    box-sizing: border-box;
    width: 100%;
    min-height: 240px;
    padding: 8px 10px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 1.45;
    resize: vertical;
  }

  .hp-hint {
    margin: 10px 0 0;
    color: var(--muted);
    font-size: 11.5px;
    line-height: 1.4;
  }

  .hp-cheats h2 {
    margin: 22px 0 4px;
    font-size: 15px;
    font-weight: 650;
  }

  .hp-table {
    margin-top: 8px;
    border-collapse: collapse;
    font-size: 12.5px;
  }

  .hp-table td {
    padding: 3px 14px 3px 0;
    vertical-align: top;
  }

  .hp-table td:first-child {
    white-space: pre;
  }

  .hp-lead :global(code),
  .hp-hint :global(code),
  .hp-table code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
  }
</style>
