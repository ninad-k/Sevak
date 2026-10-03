<script lang="ts">
  import { fetchThemeGallery, installGalleryTheme } from "./theme-ipc";
  import { GALLERY_INDEX_URL, type GalleryItem, type StoredTheme } from "./themes";

  // The opt-in online gallery. Nothing is requested until the button is
  // clicked; installing downloads one file and verifies its SHA-256 first.
  let { oninstalled }: { oninstalled: (theme: StoredTheme) => void } = $props();

  let items = $state<GalleryItem[] | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let installing = $state<string | null>(null);
  let notice = $state<string | null>(null);

  async function browse() {
    loading = true;
    error = null;
    notice = null;
    const result = await fetchThemeGallery();
    loading = false;
    if (result.ok) items = result.value;
    else error = result.error;
  }

  async function install(item: GalleryItem) {
    installing = item.id;
    error = null;
    notice = null;
    const result = await installGalleryTheme(item.id);
    installing = null;
    if (!result.ok) {
      error = `${item.name}: ${result.error}`;
      return;
    }
    items = items?.map((other) => (other.id === item.id ? { ...other, installed: true } : other)) ?? null;
    notice = `Installed ${result.value.spec.name}. It is in your themes now.`;
    oninstalled(result.value);
  }
</script>

<section class="gallery" aria-labelledby="gallery-title">
  <div class="head">
    <div class="label">
      <h3 id="gallery-title">Online themes</h3>
      <p class="hint">
        Community themes listed in Sevak's repository. Clicking <strong>Browse online themes</strong>
        makes one request to <code>{GALLERY_INDEX_URL}</code> (no account, cookies or identifying
        data). Installing downloads one theme file and checks its SHA-256 against the list before
        saving it to your themes folder. Nothing is requested until you click.
      </p>
    </div>
    <button type="button" class="btn" disabled={loading} onclick={browse}>
      {loading ? "Loading…" : items ? "Refresh list" : "Browse online themes"}
    </button>
  </div>

  {#if error}<p class="msg error" role="alert">{error}</p>{/if}
  {#if notice}<p class="msg ok" role="status">{notice}</p>{/if}

  {#if items}
    {#if items.length === 0}
      <p class="hint pad">The gallery has no themes yet.</p>
    {:else}
      <ul class="list">
        {#each items as item (item.id)}
          <li>
            <div class="info">
              <span class="name">{item.name}</span>
              <span class="hint">
                {item.author ? `by ${item.author}` : "unknown author"} · {item.mode || "theme"} ·
                SHA-256 {item.sha256.slice(0, 8)}…
              </span>
              {#if item.description}<span class="hint">{item.description}</span>{/if}
            </div>
            <button
              type="button"
              class="btn"
              disabled={installing !== null}
              aria-label="{item.installed ? 'Reinstall' : 'Install'} {item.name}"
              onclick={() => install(item)}
            >
              {installing === item.id ? "Installing…" : item.installed ? "Reinstall" : "Install"}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .gallery {
    margin-top: 12px;
    border: 1px solid var(--border);
    border-radius: 11px;
    background: var(--surface);
  }

  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 20px;
    padding: 12px 16px;
  }

  .label {
    min-width: 0;
  }

  h3 {
    margin: 0 0 2px;
    font-size: 14px;
    font-weight: 600;
  }

  .hint {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--muted);
  }

  .hint.pad {
    padding: 0 16px 12px;
  }

  code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 11.5px;
    overflow-wrap: anywhere;
  }

  .msg {
    margin: 0;
    padding: 0 16px 10px;
    font-size: 12px;
    line-height: 1.4;
  }

  .msg.error {
    color: var(--error);
  }

  .msg.ok {
    color: var(--ok);
  }

  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--border);
  }

  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 16px;
  }

  li + li {
    border-top: 1px solid var(--border);
  }

  .info {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .name {
    font-weight: 560;
  }

  .btn {
    flex: none;
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .btn:hover:not(:disabled) {
    border-color: var(--accent-strong);
  }

  .btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
