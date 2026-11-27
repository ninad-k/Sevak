<script lang="ts">
  import { onMount } from "svelte";
  import { copyDiagnostics } from "../diagnostics-ipc";
  import { formatDate, formatNumber, t } from "../i18n.svelte";
  import { getUsageStats, type UsageStats } from "../usage-ipc";
  import "./pages.css";

  let stats = $state<UsageStats | null>(null);
  let failed = $state(false);
  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    getUsageStats().then(
      (value) => (stats = value),
      () => (failed = true),
    );
    return () => clearTimeout(copyTimer);
  });

  async function copy() {
    if (!stats) return;
    const result = await copyDiagnostics(stats.text);
    if (!result.ok) return;
    copied = true;
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => (copied = false), 2000);
  }
</script>

<div class="sp-page">
  <h1>{t("usage.title")}</h1>
  <p class="sp-lead">{t("usage.lead")}</p>

  {#if failed}
    <p class="sp-note warn" role="alert">{t("usage.loadFailed")}</p>
  {:else if stats}
    <dl class="up-totals">
      <div><dt>{t("usage.total")}</dt><dd>{formatNumber(stats.total_launches)}</dd></div>
      <div><dt>{t("usage.items")}</dt><dd>{formatNumber(stats.tracked_items)}</dd></div>
      <div><dt>{t("usage.history")}</dt><dd>{formatNumber(stats.history_entries)}</dd></div>
    </dl>

    {#if stats.top.length === 0}
      <p class="sp-note">{t("usage.empty")}</p>
    {:else}
      <h2 class="sp-h2">{t("usage.top")}</h2>
      <table class="up-table">
        <thead>
          <tr>
            <th scope="col">{t("usage.item")}</th>
            <th scope="col">{t("usage.plugin")}</th>
            <th scope="col" class="num">{t("usage.count")}</th>
            <th scope="col">{t("usage.last")}</th>
          </tr>
        </thead>
        <tbody>
          {#each stats.top as item, i (i)}
            <tr>
              <td>{item.title}</td>
              <td>{item.plugin}</td>
              <td class="num">{formatNumber(item.count)}</td>
              <td>{formatDate(item.last_used * 1000, { dateStyle: "medium" })}</td>
            </tr>
          {/each}
        </tbody>
      </table>

      <h2 class="sp-h2">{t("usage.perPlugin")}</h2>
      <ul class="up-plugins">
        {#each stats.plugins as [plugin, count] (plugin)}
          <li><span>{plugin}</span><span>{formatNumber(count)}</span></li>
        {/each}
      </ul>
    {/if}

    <div class="up-actions">
      <button type="button" class="up-btn" onclick={copy}>{t("usage.copy")}</button>
      {#if copied}<span class="sp-hint" role="status">{t("usage.copied")}</span>{/if}
    </div>
    <p class="sp-note">{t("usage.privacy")}</p>
  {/if}
</div>

<style>
  .up-totals {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin: 0 0 6px;
  }

  .up-totals div {
    min-width: 130px;
    padding: 10px 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--tile);
  }

  .up-totals dt {
    color: var(--muted);
    font-size: 12px;
  }

  .up-totals dd {
    margin: 2px 0 0;
    font-size: 20px;
    font-weight: 650;
  }

  .up-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }

  .up-table th,
  .up-table td {
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
    text-align: left;
  }

  .up-table th {
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
  }

  .up-table .num {
    text-align: right;
  }

  .up-plugins {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 13px;
  }

  .up-plugins li {
    display: flex;
    justify-content: space-between;
    max-width: 320px;
    padding: 3px 0;
  }

  .up-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 16px;
  }

  .up-btn {
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
</style>
