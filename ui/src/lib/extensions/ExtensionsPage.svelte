<script lang="ts">
  // Settings > Extensions: browse the gallery (workflows, script plugins, native
  // extensions and themes), install, update, remove and switch off.
  //
  // Nothing is requested while the page is open: it shows what is installed and
  // the list saved by an earlier load. "Load" fetches the list once; an install,
  // update or removal downloads one package and only when its button is pressed.
  // Whatever is installed still waits for Sevak's Allow dialog before it runs.
  import { onMount } from "svelte";
  import Toggle from "../Toggle.svelte";
  import "../workflows/workflows.css";
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
    KIND_ORDER,
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
  const updates = $derived(updateCount(installed));

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
      notice = `The list is loaded: ${result.value.catalog?.items.length ?? 0} items.`;
    } else {
      loadError = result.error;
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
        ? `Installed the theme ${item.name}. Choose it in Settings > Appearance.`
        : `Installed ${item.name}. It does not run until you allow it, if it can run code: Sevak asks next.`,
    );

  const update = (item: { id: string; name: string }) =>
    change(item.id, () => updateItem(item.id), `Updated ${item.name}. Sevak asks again if it can run code.`);

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
      notice = `Removed ${item.name}.`;
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
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    tab = tab === "discover" ? "installed" : "discover";
    queueMicrotask(() => document.getElementById(`ext-tab-${tab}`)?.focus());
  }

  function onSearchKey(event: KeyboardEvent) {
    if (event.key === "Escape" && query !== "") {
      event.stopPropagation();
      query = "";
    }
  }

  const stateChip = (item: CatalogItem) =>
    item.state === "installed"
      ? "Installed"
      : item.state === "update_available"
        ? `Update from ${item.installed_version ?? "?"}`
        : null;

  onMount(() => {
    void reload();
    const stop = onChanged(() => void reload());
    return () => void stop.then((fn) => fn());
  });
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="wf-page ext">
  <h1>Extensions</h1>
  <p class="wf-lead">
    Workflows, script plugins, native extensions and themes from the Sevak gallery. Nothing is
    requested while you browse Settings: <strong>Load</strong> fetches the list once from the Sevak
    repository on GitHub, and <strong>Install</strong> downloads that one package, checks it against
    the checksum in the list and puts it in your folder. Whatever you install is <em>new and not
    allowed</em>: Sevak asks for your permission before anything in it runs. No other data is sent.
  </p>

  <div class="tabs" role="tablist" aria-label="Extensions" tabindex="-1" onkeydown={onTabKey}>
    <button
      type="button"
      role="tab"
      id="ext-tab-discover"
      class="tab"
      aria-selected={tab === "discover"}
      aria-controls="ext-panel-discover"
      tabindex={tab === "discover" ? 0 : -1}
      onclick={() => (tab = "discover")}>Discover</button
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
      {installed.length > 0 ? `Installed (${installed.length})` : "Installed"}
      {#if updates > 0}
        <span class="wf-chip ok dot" title="{updates} update{updates === 1 ? '' : 's'} available"
          >{updates} update{updates === 1 ? "" : "s"}</span
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
    <p class="wf-sub" role="status">Loading…</p>
  {:else if tab === "discover"}
    <div id="ext-panel-discover" role="tabpanel" aria-labelledby="ext-tab-discover">
      <div class="wf-toolbar">
        <button type="button" class="wf-btn primary" disabled={loading} onclick={load}>
          {loading ? "Loading…" : catalog ? "Refresh the list" : "Load the list"}
        </button>
        {#if catalog}
          <span class="wf-sub">
            {catalog.items.length} items
            {#if catalog.from_cache}· saved {ago(catalog.fetched_at)}{:else}· loaded {ago(catalog.fetched_at)}{/if}
            from <code class="wf-code">{hostOf(catalog.source)}</code>
            {#if catalog.skipped > 0}· {catalog.skipped} left out (this Sevak does not understand them){/if}
          </span>
        {:else}
          <span class="wf-sub">Fetches the list from GitHub (raw.githubusercontent.com).</span>
        {/if}
      </div>

      {#if loadError}
        <p class="wf-msg warn" role="alert">
          Could not reach the gallery: {loadError}
          {#if catalog}Showing the list saved {ago(catalog.fetched_at)}.{:else}You can still manage
            what is installed under Installed.{/if}
        </p>
      {/if}
      {#if catalog?.note}<p class="wf-msg" role="status">{catalog.note}</p>{/if}
      {#if catalog?.themes_error}
        <p class="wf-msg warn">Themes are not listed: {catalog.themes_error}</p>
      {/if}

      {#if catalog}
        <div class="filters">
          <label class="visually-hidden" for="ext-search">Search extensions</label>
          <input
            id="ext-search"
            class="wf-input"
            type="search"
            placeholder="Search by name, author or what it does"
            autocomplete="off"
            bind:value={query}
            onkeydown={onSearchKey}
          />
          <label class="visually-hidden" for="ext-kind">Show</label>
          <select id="ext-kind" class="wf-input kind" bind:value={filter}>
            <option value="all">All kinds</option>
            {#each KIND_ORDER as kind (kind)}
              <option value={kind}>{KIND_LABEL[kind]}s</option>
            {/each}
          </select>
        </div>

        {#if shown.length === 0}
          <p class="wf-sub">Nothing matches.</p>
        {:else}
          <ul class="rows" aria-label="Gallery items">
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
                    <span class="wf-sub line">
                      {#if item.author}by {item.author}{/if}
                      {#if item.kind === "native" && item.permissions.length > 0}
                        · declares: {item.permissions.join(", ")}
                      {/if}
                      {#if item.unavailable}· {item.unavailable}{/if}
                    </span>
                  </button>
                  {#if item.state === "available" || item.state === "update_available"}
                    {#if item.kind === "native"}
                      <button
                        type="button"
                        class="wf-btn small primary"
                        aria-label="Review {item.name} before {item.state === 'available' ? 'installing' : 'updating'}"
                        onclick={() => (expanded = item.id)}>Review…</button
                      >
                    {:else}
                      <button
                        type="button"
                        class="wf-btn small primary"
                        disabled={busy !== null}
                        aria-label="{actionLabel(item)} {item.name}"
                        onclick={() => (item.state === "available" ? install(item) : update(item))}
                      >
                        {busy === item.id ? "Working…" : actionLabel(item)}
                      </button>
                    {/if}
                  {:else}
                    <button
                      type="button"
                      class="wf-btn small"
                      disabled
                      aria-label="{item.name}: {actionLabel(item)}">{actionLabel(item)}</button
                    >
                  {/if}
                </div>

                {#if open}
                  <div class="detail" id="ext-detail-{item.id}">
                    <dl>
                      <dt>Publisher</dt>
                      <dd>{item.author || "unknown"}</dd>
                      {#if item.version}
                        <dt>Version</dt>
                        <dd>{item.version}{#if item.installed_version} (you have {item.installed_version}){/if}</dd>
                      {/if}
                      {#if item.license}
                        <dt>Licence</dt>
                        <dd>{item.license}</dd>
                      {/if}
                      {#if item.min_sevak}
                        <dt>Needs Sevak</dt>
                        <dd>{item.min_sevak} or newer</dd>
                      {/if}
                      {#if item.kind === "native"}
                        <dt>Declared permissions</dt>
                        <dd>
                          {#if item.permissions.length === 0}
                            none
                          {:else}
                            <ul class="perms">
                              {#each item.permissions as permission (permission)}
                                <li>{permissionLabel(permission)}</li>
                              {/each}
                            </ul>
                          {/if}
                          <span class="wf-sub">The author's statement. Sevak does not enforce it.</span>
                        </dd>
                        <dt>Builds</dt>
                        <dd>
                          {item.platforms.join(", ")}
                          {#if overview.platform}<span class="wf-sub"> (this computer: {overview.platform})</span>{/if}
                        </dd>
                      {/if}
                      {#if item.repository}
                        <dt>Source code</dt>
                        <dd><code class="wf-code">{item.repository}</code></dd>
                      {:else if item.homepage}
                        <dt>More</dt>
                        <dd><code class="wf-code">{item.homepage}</code></dd>
                      {/if}
                      {#if item.source}
                        <dt>Downloaded from</dt>
                        <dd><code class="wf-code">{hostOf(item.source)}</code> <span class="wf-sub">(Sevak's own repository, at this release)</span></dd>
                      {/if}
                      {#if item.sha256}
                        <dt>SHA-256</dt>
                        <dd><code class="wf-code hash">{item.sha256}</code></dd>
                      {/if}
                    </dl>
                    <p class="wf-msg" class:warn={item.kind === "native"}>{installWarning(item)}</p>
                    {#if item.state === "available" || item.state === "update_available"}
                      <button
                        type="button"
                        class="wf-btn primary"
                        disabled={busy !== null}
                        onclick={() => (item.state === "available" ? install(item) : update(item))}
                      >
                        {busy === item.id
                          ? "Working…"
                          : `${item.state === "available" ? "Install" : "Update"} ${item.kind === "native" ? "native extension" : item.name}`}
                      </button>
                    {/if}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      {:else if !loadError}
        <p class="wf-sub">
          The list has not been loaded on this computer yet. Press <strong>Load the list</strong> to
          see what is available. What you already installed is under
          <button type="button" class="wf-link" onclick={() => (tab = "installed")}>Installed</button>.
        </p>
      {/if}
    </div>
  {:else}
    <div id="ext-panel-installed" role="tabpanel" aria-labelledby="ext-tab-installed">
      <div class="wf-toolbar">
        <button type="button" class="wf-btn" onclick={() => void openPluginsFolder()}>
          Open plugins folder
        </button>
        <span class="wf-sub">
          These are the items installed from here. Plugins and workflows you added by hand are under
          Plugins and Workflows.
        </span>
      </div>
      {#if installed.length === 0}
        <p class="wf-sub">
          Nothing installed from the gallery yet. Open
          <button type="button" class="wf-link" onclick={() => (tab = "discover")}>Discover</button>
          to browse.
        </p>
      {:else}
        <ul class="rows" aria-label="Installed extensions">
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
                      <span class="wf-chip ok">Update to v{item.update_to}</span>
                    {/if}
                    {#each item.keywords as keyword (keyword)}
                      <span class="wf-chip">keyword: {keyword}</span>
                    {/each}
                  </div>
                  {#if item.problem}<p class="wf-msg error">{item.problem}</p>{/if}
                  {#if item.program_sha256}
                    <div class="wf-sub">
                      Program SHA-256 <code class="wf-code hash">{item.program_sha256}</code>
                    </div>
                  {/if}
                </div>
                <div class="actions">
                  {#if item.update_to}
                    <button
                      type="button"
                      class="wf-btn small primary"
                      disabled={busy !== null}
                      aria-label="Update {item.name} to {item.update_to}"
                      onclick={() => update(item)}>{busy === item.id ? "Working…" : "Update"}</button
                    >
                  {/if}
                  {#if item.can_review}
                    <button
                      type="button"
                      class="wf-btn small"
                      disabled={busy !== null}
                      aria-label="Review {item.name}"
                      onclick={() => review(item)}>Review…</button
                    >
                  {/if}
                  {#if item.status !== "theme" && item.status !== "broken"}
                    <Toggle
                      checked={item.enabled}
                      label="Enable {item.name}"
                      disabled={busy !== null}
                      onchange={(on) => toggle(item, on)}
                    />
                  {/if}
                  <button
                    type="button"
                    class="wf-btn small danger"
                    disabled={busy !== null}
                    aria-label="Uninstall {item.name}"
                    onclick={() => remove(item)}>Uninstall</button
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
    outline: 2px solid var(--accent);
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
