<script lang="ts">
  // Settings > Gallery: a curated list of workflows and script plugins.
  // Nothing is requested until "Load gallery" is pressed, and nothing is
  // downloaded until "Install" is pressed on an entry.
  import "./workflows.css";
  import {
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
    if (result.ok) gallery = result.value;
    else error = result.error;
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
        ? "Installed. Find it under Workflows; Sevak asks before it runs anything."
        : "Installed. Sevak asks before it runs the plugin; then it appears under Plugins.";
    // Reflect it in the list.
    const found = gallery?.entries.find((e) => e.id === entry.id);
    if (found) found.installed = true;
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
  <h1>Gallery</h1>
  <p class="wf-lead">
    Add tools to your launcher with ready-made extensions and workflows.
    Choose <strong>Load gallery</strong> to browse packages from GitHub, then install what you need.
    Script plugins ask for your permission before running. Themes are in Appearance → Theme editor.
  </p>

  {#if gallery === null}
    <div class="wf-toolbar">
      <button type="button" class="wf-btn primary" disabled={loading} onclick={load}>
        {loading ? "Loading…" : "Load gallery"}
      </button>
      <span class="wf-sub">Fetches the list from GitHub (raw.githubusercontent.com).</span>
    </div>
  {:else}
    <div class="wf-toolbar">
      <span class="wf-sub">
        {gallery.entries.length} entr{gallery.entries.length === 1 ? "y" : "ies"} from
        <code class="wf-code">{host(gallery.source)}</code>
        {#if gallery.skipped > 0}· {gallery.skipped} left out (not understood by this Sevak){/if}
      </span>
      <button type="button" class="wf-btn small" disabled={loading || installing !== null} onclick={load}>
        {loading ? "Loading…" : "Reload"}
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
    <div class="filters">
      <div class="field search">
        <label for="gallery-query">Search extensions</label>
        <input id="gallery-query" class="wf-input" type="search" bind:value={query} placeholder="Name, keyword or what you want to do…" />
      </div>
      <div class="field">
        <label for="gallery-kind">Type</label>
        <select id="gallery-kind" class="wf-input" bind:value={kind}>
          <option value="all">All types</option>
          <option value="plugin">Extensions</option>
          <option value="workflow">Workflows</option>
        </select>
      </div>
      <div class="field">
        <label for="gallery-availability">Availability</label>
        <select id="gallery-availability" class="wf-input" bind:value={availability}>
          <option value="all">All packages</option>
          <option value="available">Not installed</option>
          <option value="installed">Installed</option>
        </select>
      </div>
    </div>
    <p class="wf-sub result-count" role="status">{visible.length} of {gallery.entries.length} packages</p>
    <ul class="rows" aria-label="Gallery entries">
      {#each visible as entry (entry.id)}
        <li class="wf-card">
          <div class="card-heading">
            <span class="monogram" aria-hidden="true">{entry.name.slice(0, 2)}</span>
            <div>
              <span class="wf-chip">{entry.kind === "workflow" ? "Workflow" : "Extension · script plugin"}</span>
              <h2>{entry.name}</h2>
            </div>
          </div>
          <p class="wf-sub description">{entry.description}</p>
          <div class="meta wf-sub">
            {#if entry.author}by {entry.author}{/if}
            {#if entry.version}· version {entry.version}{/if}
          </div>
          <details class="wf-sub package-details">
            <summary>Package details</summary>
            <p>Source: <code class="wf-code">{host(entry.source)}</code></p>
            <p>SHA-256: <code class="checksum">{entry.sha256}</code></p>
            {#if entry.homepage}<p>Project: <span class="checksum">{entry.homepage}</span></p>{/if}
            <p>Downloaded only when you install. Sevak verifies the package checksum before installing.</p>
          </details>
          {#if done[entry.id]}<p class="wf-msg ok" role="status">{done[entry.id]}</p>{/if}
          {#if failed[entry.id]}<p class="wf-msg error" role="alert">{failed[entry.id]}</p>{/if}
          <div class="card-footer">
            <button
              type="button"
              class="wf-btn small"
              class:primary={!entry.installed}
              disabled={entry.installed || installing !== null}
              aria-label={entry.installed ? `${entry.name} is installed` : `Install ${entry.name}`}
              onclick={() => install(entry)}
            >
              {entry.installed ? "Installed" : installing === entry.id ? "Installing…" : "Install"}
            </button>
          </div>
        </li>
      {/each}
    </ul>
    {#if visible.length === 0}
      <div class="wf-card empty">
        <h2>{gallery.entries.length === 0 ? "The gallery has no packages yet" : "No matching extensions"}</h2>
        <p class="wf-sub">{gallery.entries.length === 0 ? "Reload later to check for new packages." : "Try another search or reset the filters."}</p>
        {#if gallery.entries.length > 0}
          <button class="wf-btn" type="button" onclick={() => { query = ""; kind = "all"; availability = "all"; }}>Reset filters</button>
        {/if}
      </div>
    {/if}
  {/if}
</div>

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
  .package-details summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .checksum { overflow-wrap: anywhere; }
  .card-footer { display: flex; justify-content: flex-end; margin-top: 6px; }
  .empty { padding: 20px; }
</style>
