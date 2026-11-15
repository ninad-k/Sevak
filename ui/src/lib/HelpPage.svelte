<script lang="ts">
  // Settings > Help: a diagnostics report for bug reports. It is made on this
  // computer, shown here exactly as it will be copied or saved, and never sent
  // anywhere.
  import { onMount } from "svelte";
  import { copyDiagnostics, getDiagnostics, saveDiagnostics } from "./diagnostics-ipc";
  import { openLogDir } from "./settings-ipc";

  let report = $state("");
  let loading = $state(true);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  // The file search filters (crates/sevak-plugins/src/files/filters.rs).
  const FILTERS: [string, string][] = [
    ["ext:pdf  ext:jpg,png", "Files with these extensions"],
    ["type:image", "Also video, audio, doc, code, archive"],
    ["size:>10mb  size:<500kb", "File size (also >=, <=, 1mb..20mb, empty)"],
    ["modified:today  modified:7d", "Changed today, or in the last 7 days (>7d: longer ago)"],
    ["modified:>2026-01-01", "Changed after that day (also >=, <, <=, 2026-01, 2026)"],
    ["created:yesterday", "Created then, where the file system records it"],
    ["in:~/Projects  in:src", "Inside that folder, or any folder of that name"],
    ["is:folder  is:file", "Folders or files only"],
    ['"annual report"', "The name contains the exact phrase"],
    ["-ext:tmp  -draft", "Leave these out"],
  ];

  async function make() {
    loading = true;
    error = null;
    notice = null;
    const result = await getDiagnostics();
    loading = false;
    if (result.ok) report = result.value;
    else error = `Could not make the report: ${result.error}`;
  }

  function say(text: string) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 4000);
  }

  async function copy() {
    const result = await copyDiagnostics(report);
    if (result.ok) say("Copied to the clipboard. Paste it into your bug report.");
    else error = `Could not copy: ${result.error}`;
  }

  async function save() {
    const result = await saveDiagnostics(report);
    if (!result.ok) error = result.error;
    else if (result.value) say(`Saved to ${result.value}.`);
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
  <h1>Help</h1>
  <p class="hp-lead">
    When something does not work, this report helps to find out why. Sevak has no telemetry, so it
    only exists here: it is made on this computer, nothing is sent anywhere, and you decide whether
    to share it. It holds the version, your system, which features are on, which plugins loaded and
    the last log lines. It does not hold what you searched for, your clipboard or snippets, the
    contents of scripts or workflows, contacts or passwords. Your user name, computer name and home
    folder are hidden. Read it before you paste it into a bug report at
    <code>github.com/ninad-k/Sevak/issues/new/choose</code>.
  </p>

  <div class="hp-toolbar">
    <button type="button" class="hp-btn primary" disabled={loading || !report} onclick={copy}>
      Copy diagnostics
    </button>
    <button type="button" class="hp-btn" disabled={loading || !report} onclick={save}>
      Save as file…
    </button>
    <button type="button" class="hp-btn" onclick={logs}>Open logs folder</button>
    <button type="button" class="hp-btn" disabled={loading} onclick={make}>
      {loading ? "Making…" : "Refresh"}
    </button>
  </div>

  {#if error}
    <p class="hp-msg error" role="alert">{error}</p>
  {:else if notice}
    <p class="hp-msg ok" role="status">{notice}</p>
  {/if}

  <label class="hp-label" for="hp-report">The report (read-only, exactly what is copied or saved)</label>
  <textarea
    id="hp-report"
    class="hp-report"
    readonly
    spellcheck="false"
    rows="18"
    value={loading && !report ? "Making the report…" : report}
  ></textarea>

  <p class="hp-hint">
    Without opening Settings: run <code>sevak --diagnostics</code> in a terminal. That report is made
    from files only, so it works even when Sevak is not running.
  </p>

  <section class="hp-cheats" aria-labelledby="hp-filters">
    <h2 id="hp-filters">File search filters</h2>
    <p class="hp-hint">
      Add these to <code>f</code>, <code>ff</code> or <code>in</code> searches, in any order. Every
      filter must match; <code>-</code> in front leaves out what follows.
    </p>
    <table class="hp-table">
      <thead class="sr-only">
        <tr><th scope="col">Filter</th><th scope="col">Meaning</th></tr>
      </thead>
      <tbody>
        {#each FILTERS as [example, meaning] (example)}
          <tr>
            <td><code>{example}</code></td>
            <td>{meaning}</td>
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

  .hp-lead code,
  .hp-hint code,
  .hp-table code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
  }
</style>
