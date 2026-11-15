<script lang="ts">
  // Settings > Extensions: browse the gallery (workflows, script plugins, native
  // extensions and themes), install, update, remove and switch off.
  //
  // Nothing is requested while the page is open: it shows what is installed and
  // the list saved by an earlier load. "Load" fetches the list once; an install,
  // update or removal downloads one package and only when its button is pressed.
  // Whatever is installed still waits for Sevak's Allow dialog before it runs.
  import { onMount } from "svelte";
  import MarketplaceLinks from "../MarketplaceLinks.svelte";
  import Toggle from "../Toggle.svelte";
  import "../workflows/workflows.css";
  import { t, tHtml, type MessageKey, type Params } from "../i18n.svelte";
  import {
    getOverview,
    installItem,
    onChanged,
    openPluginsFolder,
    refreshCatalog,
    reviewItem,
    setItemEnabled,
    uninstallItem,
    updateItem,
    type CatalogItem,
    type InstalledItem,
    type Overview,
    type Result,
  } from "./extensions-ipc";
  import {
    KIND_LABEL,
    KIND_NOUN,
    KIND_ORDER,
    KIND_PLURAL,
    actionLabel,
    ago,
    hostOf,
    installWarning,
    permissionLabel,
    statusLabel,
    updateCount,
    visibleItems,
    type KindFilter,
  } from "./model";

  type Tab = "discover" | "installed";

  let overview = $state<Overview | null>(null);
  let tab = $state<Tab>("discover");
  let query = $state("");
  let filter = $state<KindFilter>("all");
  let loading = $state(false);
  /** The id being installed, updated, removed or switched. */
  let busy = $state<string | null>(null);
  /** Why the list could not be loaded just now (the saved list, if any, stays). */
  let loadError = $state<string | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let expanded = $state<string | null>(null);

  const catalog = $derived(overview?.catalog ?? null);
  const shown = $derived(visibleItems(catalog?.items ?? [], filter, query));
  const installed = $derived(overview?.installed ?? []);
  const unrecorded = $derived(overview?.unrecorded ?? []);
  /** The replacement's name when the loaded list has it, otherwise its id. */
  const replacementName = (id: string) => catalog?.items.find((i) => i.id === id)?.name ?? id;
  const updates = $derived(updateCount(installed));

  /** A message around a `{link}` placeholder, split so a button can go in its place. */
  function aroundLink(key: MessageKey, params?: Params): [string, string] {
    const [before, after = ""] = t(key, params).split("{link}");
    return [before, after];
  }

  function take(result: Result<Overview>): boolean {
    if (!result.ok) {
      error = result.error;
      return false;
    }
    overview = result.value;
    return true;
  }

  async function reload() {
    const result = await getOverview();
    if (result.ok) overview = result.value;
    else error = result.error;
  }

  async function load() {
    loading = true;
    error = null;
    notice = null;
    const result = await refreshCatalog();
    loading = false;
    if (result.ok) {
      overview = result.value;
      loadError = null;
      notice = t("extensions.loaded", { count: result.value.catalog?.items.length ?? 0 });
    } else {
      loadError = result.error;
      // Advisory refresh may have succeeded or failed before catalog loading.
      // Read the resulting local status even if the pinned catalog is offline.
      await reload();
    }
  }

  /** Runs a change on `item`, then shows what it did. */
  async function change(id: string, work: () => Promise<Result<Overview>>, done: string) {
    busy = id;
    error = null;
    notice = null;
    const result = await work();
    busy = null;
    if (take(result)) notice = done;
  }

  const install = (item: CatalogItem) =>
    change(
      item.id,
      () => installItem(item.id),
      item.kind === "theme"
        ? t("extensions.installedTheme", { name: item.name })
        : t("extensions.installedItem", { name: item.name }),
    );

  const update = (item: { id: string; name: string }) =>
    change(item.id, () => updateItem(item.id), t("extensions.updated", { name: item.name }));

  async function remove(item: InstalledItem) {
    busy = item.id;
    error = null;
    notice = null;
    const result = await uninstallItem(item.id);
    busy = null;
    if (!result.ok) {
      error = result.error;
    } else if (result.value) {
      overview = result.value;
      notice = t("extensions.removed", { name: item.name });
    }
  }

  async function toggle(item: InstalledItem, on: boolean) {
    busy = item.id;
    error = null;
    const result = await setItemEnabled(item.id, on);
    busy = null;
    take(result);
  }

  async function review(item: InstalledItem) {
    busy = item.id;
    error = null;
    const result = await reviewItem(item.id);
    busy = null;
    if (!result.ok) error = result.error;
    await reload();
  }

  function toggleDetails(id: string) {
    expanded = expanded === id ? null : id;
  }

  /** Escape inside an open card closes it and puts the focus back on its title. */
  function onWindowKey(event: KeyboardEvent) {
    if (event.key !== "Escape" || expanded === null) return;
    const card = document.getElementById(`ext-head-${expanded}`)?.closest("li");
    if (!card?.contains(document.activeElement)) return;
    const id = expanded;
    expanded = null;
    (document.getElementById(`ext-head-${id}`) as HTMLElement | null)?.focus();
  }

  function onTabKey(event: KeyboardEvent) {
    const next =
      event.key === "Home"
        ? "discover"
        : event.key === "End"
          ? "installed"
          : event.key === "ArrowLeft" || event.key === "ArrowRight"
            ? tab === "discover"
              ? "installed"
              : "discover"
            : null;
    if (next === null) return;
    event.preventDefault();
    showTab(next);
  }

  /** Switches tabs and puts the focus on the new tab (the old panel, focus and all, goes away). */
  function showTab(next: typeof tab) {
    tab = next;
    queueMicrotask(() => document.getElementById(`ext-tab-${next}`)?.focus());
  }

  function onSearchKey(event: KeyboardEvent) {
    if (event.key === "Escape" && query !== "") {
      event.stopPropagation();
      query = "";
    }
  }

  const stateChip = (item: CatalogItem) =>
    item.state === "installed"
      ? t("extensions.installed")
      : item.state === "update_available"
        ? t("extensions.updateFrom", { version: item.installed_version ?? "?" })
        : null;

  onMount(() => {
    void reload();
    const stop = onChanged(() => void reload());
    return () => void stop.then((fn) => fn());
  });
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="wf-page ext">
  <h1>{t("extensions.title")}</h1>
  <p class="wf-lead">{@html tHtml("extensions.lead")}</p>
  <MarketplaceLinks />
  <p class="wf-msg" class:warn={overview?.advisories?.state !== "verified"} role="status">
    {overview?.advisories?.message ?? t("extensions.statusUnknown")}
  </p>

  <div class="tabs" role="tablist" aria-label={t("extensions.tabs")} tabindex="-1" onkeydown={onTabKey}>
    <button
      type="button"
      role="tab"
      id="ext-tab-discover"
      class="tab"
      aria-selected={tab === "discover"}
      aria-controls="ext-panel-discover"
      tabindex={tab === "discover" ? 0 : -1}
      onclick={() => (tab = "discover")}>{t("extensions.discover")}</button
    >
    <button
      type="button"
      role="tab"
      id="ext-tab-installed"
      class="tab"
      aria-selected={tab === "installed"}
      aria-controls="ext-panel-installed"
      tabindex={tab === "installed" ? 0 : -1}
      onclick={() => (tab = "installed")}
    >
      {installed.length > 0
        ? t("extensions.installedCount", { count: installed.length })
        : t("extensions.installed")}
      {#if updates > 0}
        <span class="wf-chip ok dot" title={t("extensions.updatesAvailable", { count: updates })}
          >{t("extensions.updates", { count: updates })}</span
        >
      {/if}
    </button>
  </div>

  <div class="live" role="status" aria-live="polite">
    {#if notice}<p class="wf-msg ok">{notice}</p>{/if}
  </div>
  {#if error}
    <p class="wf-msg error" role="alert">{error}</p>
  {/if}

  {#if overview === null}
    <p class="wf-sub" role="status">{t("common.loading")}</p>
  {:else if tab === "discover"}
    <div id="ext-panel-discover" role="tabpanel" aria-labelledby="ext-tab-discover">
      <div class="wf-toolbar">
        <button type="button" class="wf-btn primary" disabled={loading || busy !== null} onclick={load}>
          {loading ? t("common.loading") : catalog ? t("extensions.refresh") : t("extensions.load")}
        </button>
        {#if catalog}
          <span class="wf-sub">
            {t("extensions.items", { count: catalog.items.length })}
            {#if catalog.from_cache}· {t("extensions.savedAgo", { ago: ago(catalog.fetched_at) })}{:else}· {t("extensions.loadedAgo", { ago: ago(catalog.fetched_at) })}{/if}
            {@html tHtml("extensions.fromHost", { host: hostOf(catalog.source) })}
            {#if catalog.index_signature === "verified"}· <span title={t("extensions.signedIndexTitle")}>{t("extensions.signedIndex")}</span>{/if}
            {#if catalog.skipped > 0}· {t("extensions.skipped", { count: catalog.skipped })}{/if}
          </span>
        {:else}
          <span class="wf-sub">{t("extensions.fetches")}</span>
        {/if}
      </div>

      {#if loadError}
        <p class="wf-msg warn" role="alert">
          {t("extensions.unreachable", { error: loadError })}
          {#if catalog}{t("extensions.showingSaved", { ago: ago(catalog.fetched_at) })}{:else}{t("extensions.manageInstalled")}{/if}
        </p>
      {/if}
      {#if catalog?.note}<p class="wf-msg" role="status">{catalog.note}</p>{/if}
      {#if catalog?.themes_error}
        <p class="wf-msg warn">{t("extensions.themesError", { error: catalog.themes_error })}</p>
      {/if}

      {#if catalog}
        <div class="filters">
          <label class="visually-hidden" for="ext-search">{t("extensions.search")}</label>
          <input
            id="ext-search"
            class="wf-input"
            type="search"
            placeholder={t("extensions.searchPlaceholder")}
            autocomplete="off"
            bind:value={query}
            onkeydown={onSearchKey}
          />
          <label class="visually-hidden" for="ext-kind">{t("extensions.show")}</label>
          <select id="ext-kind" class="wf-input kind" bind:value={filter}>
            <option value="all">{t("extensions.allKinds")}</option>
            {#each KIND_ORDER as kind (kind)}
              <option value={kind}>{KIND_PLURAL[kind]}</option>
            {/each}
          </select>
        </div>

        {#if shown.length === 0}
          <p class="wf-sub">{t("extensions.nothing")}</p>
        {:else}
          <ul class="rows" aria-label={t("extensions.listLabel")}>
            {#each shown as item (item.id)}
              {@const open = expanded === item.id}
              <li class="wf-card">
                <div class="wf-row">
                  <button
                    type="button"
                    class="head wf-grow"
                    id="ext-head-{item.id}"
                    aria-expanded={open}
                    aria-controls="ext-detail-{item.id}"
                    onclick={() => toggleDetails(item.id)}
                  >
                    <span class="wf-title">{item.name}</span>
                    <span class="chips">
                      <span class="wf-chip" class:warn={item.kind === "native"}>{KIND_LABEL[item.kind]}</span>
                      {#if item.version}<span class="wf-chip">v{item.version}</span>{/if}
                      {#if stateChip(item)}
                        <span class="wf-chip ok">{stateChip(item)}</span>
                      {/if}
                    </span>
                    <span class="wf-sub line">{item.description}</span>
                    {#if item.deprecated}
                      <span class="wf-sub line">
                        {item.replaced_by
                          ? t("extensions.deprecatedReplaced", { reason: item.deprecated, name: replacementName(item.replaced_by) })
                          : t("extensions.deprecated", { reason: item.deprecated })}
                      </span>
                    {/if}
                    {#if item.revocation}<span class="wf-msg error">{t("extensions.revoked", { reason: item.revocation.reason })}</span>{/if}
                    <span class="wf-sub line">
                      {#if item.author}{t("extensions.by", { author: item.author })}{/if}
                      {#if item.kind === "native" && item.permissions.length > 0}
                        · {t("extensions.declares", { list: item.permissions.join(", ") })}
                      {/if}
                      {#if item.unavailable}· {item.unavailable}{/if}
                    </span>
                  </button>
                  {#if !item.revocation && (item.state === "available" || item.state === "update_available")}
                    {#if item.kind === "native"}
                      <button
                        type="button"
                        class="wf-btn small primary"
                        aria-label={item.state === "available"
                          ? t("extensions.reviewBeforeInstall", { name: item.name })
                          : t("extensions.reviewBeforeUpdate", { name: item.name })}
                        aria-expanded={open}
                        aria-controls="ext-detail-{item.id}"
                        onclick={() => (expanded = item.id)}>{t("extensions.review")}</button
                      >
                    {:else}
                      <button
                        type="button"
                        class="wf-btn small primary"
                        disabled={busy !== null}
                        aria-label={busy === item.id
                          ? t("extensions.workingOn", { name: item.name })
                          : t("extensions.actionAria", { action: actionLabel(item), name: item.name })}
                        onclick={() => (item.state === "available" ? install(item) : update(item))}
                      >
                        {busy === item.id ? t("extensions.working") : actionLabel(item)}
                      </button>
                    {/if}
                  {:else}
                    <button
                      type="button"
                      class="wf-btn small"
                      disabled
                      aria-label={t("extensions.disabledAria", { name: item.name, action: actionLabel(item) })}
                      >{actionLabel(item)}</button
                    >
                  {/if}
                </div>

                {#if open}
                  <div class="detail" id="ext-detail-{item.id}">
                    <dl>
                      <dt>{t("extensions.publisher")}</dt>
                      <dd>{item.author || t("extensions.unknown")}</dd>
                      {#if item.version}
                        <dt>{t("extensions.version")}</dt>
                        <dd>
                          {item.installed_version
                            ? t("extensions.youHave", { version: item.version, installed: item.installed_version })
                            : item.version}
                        </dd>
                      {/if}
                      {#if item.license}
                        <dt>{t("extensions.licence")}</dt>
                        <dd>{item.license}</dd>
                      {/if}
                      {#if item.min_sevak}
                        <dt>{t("extensions.needsSevak")}</dt>
                        <dd>{t("extensions.orNewer", { version: item.min_sevak })}</dd>
                      {/if}
                      {#if item.kind === "native"}
                        <dt>{t("extensions.permissions")}</dt>
                        <dd>
                          {#if item.permissions.length === 0}
                            {t("extensions.none")}
                          {:else}
                            <ul class="perms">
                              {#each item.permissions as permission (permission)}
                                <li>{permissionLabel(permission)}</li>
                              {/each}
                            </ul>
                          {/if}
                          <span class="wf-sub">{t("extensions.authorsStatement")}</span>
                        </dd>
                        <dt>{t("extensions.builds")}</dt>
                        <dd>
                          {item.platforms.join(", ")}
                          {#if overview.platform}<span class="wf-sub"> {t("extensions.thisComputer", { platform: overview.platform })}</span>{/if}
                        </dd>
                      {/if}
                      {#if item.repository}
                        <dt>{t("extensions.sourceCode")}</dt>
                        <dd><code class="wf-code">{item.repository}</code></dd>
                      {:else if item.homepage}
                        <dt>{t("extensions.more")}</dt>
                        <dd><code class="wf-code">{item.homepage}</code></dd>
                      {/if}
                      {#if item.source}
                        <dt>{t("extensions.downloadedFrom")}</dt>
                        <dd><code class="wf-code">{hostOf(item.source)}</code> <span class="wf-sub">{t("extensions.ownRepo")}</span></dd>
                      {/if}
                      {#if item.sha256}
                        <dt>SHA-256</dt>
                        <dd><code class="wf-code hash">{item.sha256}</code></dd>
                      {/if}
                    </dl>
                    <p class="wf-msg" class:warn={item.kind === "native"}>{installWarning(item)}</p>
                    {#if !item.revocation && (item.state === "available" || item.state === "update_available")}
                      <button
                        type="button"
                        class="wf-btn primary"
                        disabled={busy !== null}
                        onclick={() => (item.state === "available" ? install(item) : update(item))}
                      >
                        {busy === item.id
                          ? t("extensions.working")
                          : item.kind === "native"
                            ? item.state === "available"
                              ? t("extensions.installNative")
                              : t("extensions.updateNative")
                            : item.state === "available"
                              ? t("extensions.installName", { name: item.name })
                              : t("extensions.updateName", { name: item.name })}
                      </button>
                    {/if}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      {:else if !loadError}
        {@const [before, after] = aroundLink("extensions.alreadyInstalled")}
        <p class="wf-sub">
          {@html tHtml("extensions.notLoadedYet")}
          {before}<button type="button" class="wf-link" onclick={() => showTab("installed")}
            >{t("extensions.installed")}</button
          >{after}
        </p>
      {/if}
    </div>
  {:else}
    <div id="ext-panel-installed" role="tabpanel" aria-labelledby="ext-tab-installed">
      <div class="wf-toolbar">
        <button type="button" class="wf-btn" disabled={loading || busy !== null} onclick={load}>{t("extensions.checkAdvisories")}</button>
        <button type="button" class="wf-btn" onclick={() => void openPluginsFolder()}>
          {t("extensions.openPlugins")}
        </button>
        <span class="wf-sub">{t("extensions.installedNote")}</span>
      </div>
      {#if loadError}<p class="wf-msg warn" role="alert">{t("extensions.refreshFailed", { error: loadError })}</p>{/if}
      {#if unrecorded.length > 0}
        <ul class="rows" aria-label={t("extensions.unrecordedLabel")}>
          {#each unrecorded as copy (`${copy.kind}:${copy.id}:${copy.path}`)}
            <li class="wf-card">
              <div class="wf-title">{copy.name}</div>
              <p class="wf-msg error" role="alert">
                {t("extensions.unrecordedRevoked", { reason: copy.revocation.reason, kind: KIND_NOUN[copy.kind] })}
                <code class="wf-code">{copy.path}</code>
              </p>
              {#if copy.revocation.advisory_url}<p class="wf-sub">{t("extensions.advisory", { url: copy.revocation.advisory_url })}</p>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if installed.length === 0}
        {@const [before, after] = aroundLink("extensions.nothingInstalled")}
        <p class="wf-sub">
          {before}<button type="button" class="wf-link" onclick={() => showTab("discover")}
            >{t("extensions.discover")}</button
          >{after}
        </p>
      {:else}
        <ul class="rows" aria-label={t("extensions.installedLabel")}>
          {#each installed as item (item.id)}
            <li class="wf-card">
              <div class="wf-row">
                <div class="wf-grow">
                  <div class="wf-title">{item.name}</div>
                  <div class="chips">
                    <span class="wf-chip" class:warn={item.kind === "native"}>{KIND_LABEL[item.kind]}</span>
                    {#if item.version}<span class="wf-chip">v{item.version}</span>{/if}
                    <span
                      class="wf-chip"
                      class:ok={item.status === "ready" || item.status === "theme"}
                      class:warn={item.status === "waiting" || item.status === "disabled"}
                      class:error={item.status === "broken"}>{statusLabel(item)}</span
                    >
                    {#if item.update_to}
                      <span class="wf-chip ok">{t("extensions.updateTo", { version: item.update_to })}</span>
                    {/if}
                    {#each item.keywords as keyword (keyword)}
                      <span class="wf-chip">{t("extensions.keyword", { keyword })}</span>
                    {/each}
                  </div>
                  {#if item.problem}<p class="wf-msg error">{item.problem}</p>{/if}
                  {#if item.deprecated}
                    <p class="wf-msg warn">
                      {item.replaced_by
                        ? t("extensions.deprecatedReplaced", { reason: item.deprecated, name: replacementName(item.replaced_by) })
                        : t("extensions.deprecated", { reason: item.deprecated })}
                    </p>
                  {/if}
                  {#if item.revocation}
                    <p class="wf-msg error" role="alert">{t("extensions.revokedInstalled", { reason: item.revocation.reason })}</p>
                    {#if item.revocation.advisory_url}<p class="wf-sub">{t("extensions.advisory", { url: item.revocation.advisory_url })}</p>{/if}
                  {/if}
                  {#if item.program_sha256}
                    <div class="wf-sub">
                      {t("extensions.programSha")} <code class="wf-code hash">{item.program_sha256}</code>
                    </div>
                  {/if}
                </div>
                <div class="actions">
                  {#if item.update_to}
                    <button
                      type="button"
                      class="wf-btn small primary"
                      disabled={busy !== null}
                      aria-label={t("extensions.updateAria", { name: item.name, version: item.update_to })}
                      onclick={() => update(item)}
                      >{busy === item.id ? t("extensions.working") : t("extensions.update")}</button
                    >
                  {/if}
                  {#if item.can_review && !item.revocation}
                    <button
                      type="button"
                      class="wf-btn small"
                      disabled={busy !== null}
                      aria-label={t("extensions.reviewAria", { name: item.name })}
                      onclick={() => review(item)}>{t("extensions.review")}</button
                    >
                  {/if}
                  {#if item.status !== "theme" && item.status !== "broken"}
                    <Toggle
                      checked={item.enabled}
                      label={t("extensions.enable", { name: item.name })}
                      disabled={busy !== null}
                      onchange={(on) => toggle(item, on)}
                    />
                  {/if}
                  <button
                    type="button"
                    class="wf-btn small danger"
                    disabled={busy !== null}
                    aria-label={t("extensions.uninstallAria", { name: item.name })}
                    onclick={() => remove(item)}>{t("extensions.uninstall")}</button
                  >
                </div>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</div>

<style>
  .rows {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 12px;
    border-bottom: 1px solid var(--border);
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 7px 14px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .tab[aria-selected="true"] {
    border-bottom-color: var(--accent-strong);
    color: var(--fg);
    font-weight: 600;
  }

  .tab:focus-visible,
  .head:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .dot {
    margin-left: 2px;
  }

  .live {
    min-height: 0;
  }

  .filters {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
  }

  .filters .kind {
    width: auto;
    min-width: 150px;
  }

  .head {
    display: block;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin: 4px 0;
  }

  .line {
    display: block;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .detail {
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 14px;
    margin: 0 0 8px;
    font-size: 12.5px;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .perms {
    margin: 0 0 2px;
    padding-left: 16px;
  }

  .hash {
    user-select: all;
    word-break: break-all;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
