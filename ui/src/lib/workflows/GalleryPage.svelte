<script lang="ts">
  // Settings > Gallery: a curated list of workflows and script plugins.
  // Nothing is requested until "Load gallery" is pressed, and nothing is
  // downloaded until "Install" is pressed on an entry.
  import "./workflows.css";
  import MarketplaceLinks from "../MarketplaceLinks.svelte";
  import { formatList, t, tHtml } from "../i18n.svelte";
  import ConfigureForm from "./ConfigureForm.svelte";
  import {
    getWorkflowConfig,
    installGalleryEntry,
    loadGallery,
    type Gallery,
    type GalleryEntry,
  } from "./ipc";

  let gallery = $state<Gallery | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let installing = $state<string | null>(null);
  let done = $state<Record<string, string>>({});
  let failed = $state<Record<string, string>>({});
  /** A workflow just installed that needs its settings: the form opens at once. */
  let configuring = $state<string | null>(null);
  let query = $state("");
  let kind = $state("all");
  let availability = $state("all");
  const visible = $derived((gallery?.entries ?? []).filter(entry => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const text = `${entry.name} ${entry.description} ${entry.author}`.toLowerCase();
    return words.every(word => text.includes(word)) &&
      (kind === "all" || entry.kind === kind) &&
      (availability === "all" || entry.installed === (availability === "installed"));
  }));

  async function load() {
    loading = true;
    error = null;
    const result = await loadGallery();
    loading = false;
    if (result.ok) {
      gallery = result.value;
      error = result.value.load_error ?? null;
    }
    else {
      error = result.error;
      if (gallery) gallery.advisories = { state: "unavailable", message: t("gallery.refreshFailed"), issued_at: gallery.advisories?.issued_at ?? null, expires_at: gallery.advisories?.expires_at ?? null };
    }
  }

  async function install(entry: GalleryEntry) {
    installing = entry.id;
    delete failed[entry.id];
    const result = await installGalleryEntry(entry.id);
    installing = null;
    if (!result.ok) {
      failed[entry.id] = result.error;
      return;
    }
    done[entry.id] =
      entry.kind === "workflow"
        ? t("gallery.installedWorkflow")
        : t("gallery.installedPlugin");
    // Reflect it in the list.
    const found = gallery?.entries.find((e) => e.id === entry.id);
    if (found) found.installed = true;
    if (entry.kind === "workflow") await configureIfNeeded(result.value.folder);
  }

  /** Opens the settings form when the new workflow cannot run without it. */
  async function configureIfNeeded(folder: string) {
    const form = await getWorkflowConfig(folder);
    if (form.ok && form.value.missing.length > 0) configuring = folder;
  }

  /** The replacement's name when it is listed, otherwise its id. */
  const replacementName = (id: string) => gallery?.entries.find((e) => e.id === id)?.name ?? id;
  /** On disk, but Sevak has no record of which version it installed. */
  const unrecorded = (entry: GalleryEntry) => entry.installed && !entry.installed_version;
  const revokedVersions = (versions: string[]) =>
    versions.includes("*")
      ? t("gallery.everyVersion")
      : t("gallery.versions", { count: versions.length, list: formatList(versions) });
  /** What the page says about a copy whose installed version is not on record. */
  function unknownCheck(entry: GalleryEntry): string {
    return entry.installed_path
      ? t("gallery.unknownCheckAt", { path: entry.installed_path })
      : t("gallery.unknownCheck");
  }
  function unrecordedRevoked(entry: GalleryEntry): string {
    const notice = entry.installed_revocation!;
    const params = { versions: revokedVersions(notice.versions), reason: notice.reason };
    return entry.installed_path
      ? t("gallery.unrecordedRevokedAt", { ...params, path: entry.installed_path })
      : t("gallery.unrecordedRevoked", params);
  }

  const host = (url: string) => {
    try {
      return new URL(url).host;
    } catch {
      return url;
    }
  };
</script>

<div class="wf-page">
  <h1>{t("gallery.title")}</h1>
  <p class="wf-lead">{@html tHtml("gallery.lead")}</p>
  <MarketplaceLinks />

  {#if gallery === null}
    <div class="wf-toolbar">
      <button type="button" class="wf-btn primary" disabled={loading} onclick={load}>
        {loading ? t("common.loading") : t("gallery.load")}
      </button>
      <span class="wf-sub">{t("gallery.fetches")}</span>
    </div>
  {:else}
    <div class="wf-toolbar">
      <span class="wf-sub">
        {@html tHtml("gallery.entries", { count: gallery.entries.length, host: host(gallery.source) })}
        {#if gallery.skipped > 0}· {t("gallery.skipped", { count: gallery.skipped })}{/if}
      </span>
      <button type="button" class="wf-btn small" disabled={loading || installing !== null} onclick={load}>
        {loading ? t("common.loading") : t("gallery.reload")}
      </button>
    </div>
  {/if}

  {#if error}
    <p class="wf-msg error" role="alert">{error}</p>
  {/if}
  {#if gallery?.note}
    <p class="wf-msg" role="status">{gallery.note}</p>
  {/if}

  {#if gallery}
    <p class="wf-msg" class:warn={gallery.advisories?.state !== "verified"} role="status">
      {gallery.advisories?.message ?? t("gallery.statusUnknown")}
    </p>
    <div class="filters">
      <div class="field search">
        <label for="gallery-query">{t("gallery.search")}</label>
        <input id="gallery-query" class="wf-input" type="search" bind:value={query} placeholder={t("gallery.searchPlaceholder")} />
      </div>
      <div class="field">
        <label for="gallery-kind">{t("gallery.type")}</label>
        <select id="gallery-kind" class="wf-input" bind:value={kind}>
          <option value="all">{t("gallery.allTypes")}</option>
          <option value="plugin">{t("gallery.extensions")}</option>
          <option value="workflow">{t("gallery.workflows")}</option>
        </select>
      </div>
      <div class="field">
        <label for="gallery-availability">{t("gallery.availability")}</label>
        <select id="gallery-availability" class="wf-input" bind:value={availability}>
          <option value="all">{t("gallery.allPackages")}</option>
          <option value="available">{t("gallery.notInstalled")}</option>
          <option value="installed">{t("gallery.installed")}</option>
        </select>
      </div>
    </div>
    <p class="wf-sub result-count" role="status">
      {t("gallery.shownOf", { shown: visible.length, count: gallery.entries.length })}
    </p>
    <ul class="rows" aria-label={t("gallery.listLabel")}>
      {#each visible as entry (entry.id)}
        <li class="wf-card">
          <div class="card-heading">
            <span class="monogram" aria-hidden="true">{entry.name.slice(0, 2)}</span>
            <div>
              <span class="wf-chip">{entry.kind === "workflow" ? t("gallery.kindWorkflow") : t("gallery.kindPlugin")}</span>
              <h2>{entry.name}</h2>
            </div>
          </div>
          <p class="wf-sub description">{entry.description}</p>
          {#if entry.deprecated}
            <p class="wf-msg warn">
              {entry.replaced_by
                ? t("gallery.deprecatedReplaced", { reason: entry.deprecated, name: replacementName(entry.replaced_by) })
                : t("gallery.deprecated", { reason: entry.deprecated })}
            </p>
          {/if}
          {#if entry.revocation}<p class="wf-msg error" role="alert">{t("gallery.listedRevoked", { reason: entry.revocation.reason })} {unrecorded(entry) && !entry.installed_revocation ? unknownCheck(entry) : ""}</p>{/if}
          {#if entry.installed_revocation}
            {#if unrecorded(entry)}
              <p class="wf-msg error" role="alert">{unrecordedRevoked(entry)}</p>
            {:else}
              <p class="wf-msg error" role="alert">{t("gallery.installedRevoked", { reason: entry.installed_revocation.reason })}</p>
            {/if}
          {/if}
          <div class="meta wf-sub">
            {#if entry.author}{t("gallery.by", { author: entry.author })}{/if}
            {#if entry.version}· {t("gallery.version", { version: entry.version })}{/if}
          </div>
          <details class="wf-sub package-details">
            <summary>{t("gallery.details")}</summary>
            <p>{t("gallery.source")} <code class="wf-code">{host(entry.source)}</code></p>
            <p>SHA-256: <code class="checksum">{entry.sha256}</code></p>
            {#if entry.homepage}<p>{t("gallery.project")} <span class="checksum">{entry.homepage}</span></p>{/if}
            <p>{t("gallery.detailsNote")}</p>
          </details>
          {#if done[entry.id]}<p class="wf-msg ok" role="status">{done[entry.id]}</p>{/if}
          {#if failed[entry.id]}<p class="wf-msg error" role="alert">{failed[entry.id]}</p>{/if}
          <div class="card-footer">
            <button
              type="button"
              class="wf-btn small"
              class:primary={!entry.installed}
              disabled={entry.installed || installing !== null || !!entry.deprecated || !!entry.revocation}
              aria-label={entry.installed
                ? t("gallery.installedAria", { name: entry.name })
                : installing === entry.id
                  ? t("gallery.installingAria", { name: entry.name })
                  : t("gallery.installAria", { name: entry.name })}
              onclick={() => install(entry)}
            >
              {entry.installed
                ? t("gallery.installed")
                : installing === entry.id
                  ? t("gallery.installing")
                  : t("gallery.install")}
            </button>
          </div>
        </li>
      {/each}
    </ul>
    {#if visible.length === 0}
      <div class="wf-card empty">
        <h2>{gallery.entries.length === 0 ? t("gallery.emptyGallery") : t("gallery.noMatches")}</h2>
        <p class="wf-sub">{gallery.entries.length === 0 ? t("gallery.emptyGalleryHint") : t("gallery.noMatchesHint")}</p>
        {#if gallery.entries.length > 0}
          <button class="wf-btn" type="button" onclick={() => { query = ""; kind = "all"; availability = "all"; }}>{t("gallery.resetFilters")}</button>
        {/if}
      </div>
    {/if}
  {/if}
</div>

{#if configuring}
  <ConfigureForm folder={configuring} onclose={() => (configuring = null)} />
{/if}

<style>
  .rows {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(290px, 100%), 1fr));
    gap: 12px;
  }

  .meta {
    margin-top: 4px;
  }

  .filters { display: flex; flex-wrap: wrap; gap: 10px; margin: 18px 0 10px; }
  .filters .field { display: grid; gap: 5px; font-size: 12px; min-width: 130px; }
  .filters .search { flex: 1; min-width: 210px; }
  .result-count { margin-bottom: 12px; }
  .rows .wf-card { display: flex; flex-direction: column; margin: 0; min-width: 0; padding: 16px; }
  .card-heading { display: flex; align-items: center; gap: 12px; }
  .monogram { display: grid; place-items: center; flex-shrink: 0; width: 44px; height: 44px; border-radius: 12px; background: var(--selected); color: var(--fg); font-size: 19px; font-weight: 650; }
  h2 { font-size: 15px; margin: 6px 0; font-weight: 600; }
  .description { margin: 12px 0; line-height: 1.6; flex: 1; }
  .package-details { margin: 12px 0; }
  .package-details summary { cursor: pointer; }
  .package-details summary:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
  .checksum { overflow-wrap: anywhere; }
  .card-footer { display: flex; justify-content: flex-end; margin-top: 6px; }
  .empty { padding: 20px; }
</style>
