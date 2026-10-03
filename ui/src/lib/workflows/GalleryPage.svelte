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
    Ready-made workflows and script plugins. The gallery is a short list kept in the Sevak
    repository. Nothing is requested while you browse Settings: pressing <strong>Load gallery</strong>
    fetches the list once, and pressing <strong>Install</strong> downloads that one package, checks
    it against the checksum in the list, and unpacks it into your workflows or plugins folder.
    Sevak then still asks for your permission before anything in it runs. No other data is sent.
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
      <button type="button" class="wf-btn small" disabled={loading} onclick={load}>
        {loading ? "Loading…" : "Reload"}
      </button>
    </div>
  {/if}

  {#if error}
    <p class="wf-msg error" role="alert">{error}</p>
  {/if}

  {#if gallery}
    <ul class="rows" aria-label="Gallery entries">
      {#each gallery.entries as entry (entry.id)}
        <li class="wf-card">
          <div class="wf-row">
            <div class="wf-grow">
              <div class="wf-title">
                {entry.name}
                <span class="wf-chip kind">{entry.kind === "workflow" ? "Workflow" : "Script plugin"}</span>
              </div>
              <div class="wf-sub">{entry.description}</div>
              <div class="meta wf-sub">
                {#if entry.author}by {entry.author}{/if}
                {#if entry.version}· version {entry.version}{/if}
                · from <code class="wf-code">{host(entry.source)}</code>
                · sha256 <code class="wf-code" title={entry.sha256}>{entry.sha256.slice(0, 12)}…</code>
              </div>
              {#if done[entry.id]}<p class="wf-msg ok" role="status">{done[entry.id]}</p>{/if}
              {#if failed[entry.id]}<p class="wf-msg error" role="alert">{failed[entry.id]}</p>{/if}
            </div>
            <button
              type="button"
              class="wf-btn small"
              class:primary={!entry.installed}
              disabled={entry.installed || installing !== null}
              onclick={() => install(entry)}
            >
              {entry.installed ? "Installed" : installing === entry.id ? "Installing…" : "Install"}
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .rows {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .meta {
    margin-top: 4px;
  }

  .kind {
    margin-left: 6px;
    vertical-align: 1px;
  }
</style>
