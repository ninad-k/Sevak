<script lang="ts">
  // Settings > Backup & restore. Self-contained: it talks to the backend through
  // backup-ipc.ts and is not part of the settings form (nothing here is saved by
  // the Save button of the other pages).
  import { onMount } from "svelte";
  import Toggle from "../Toggle.svelte";
  import {
    applyRestore,
    backupNow,
    getContents,
    getOverview,
    openBackupFolder,
    pickBackup,
    previewRestore,
    saveBackupAs,
    setAuto,
    undoRestore,
  } from "./backup-ipc";
  import {
    changeLabel,
    changesSomething,
    chosen,
    clampKeep,
    defaultTicks,
    formatBytes,
    formatWhen,
    platformName,
    removals,
    SCHEDULES,
    summarize,
    type AutoSettings,
    type Contents,
    type Inspected,
    type Mode,
    type Overview,
    type RestorePreview,
    type RestoreResult,
  } from "./backup-model";

  /** Called after a restore or an undo changed the settings, so the other pages reload them. */
  let { onrestored }: { onrestored?: () => void | Promise<void> } = $props();

  type Msg = { kind: "ok" | "error"; text: string };

  let overview = $state<Overview | null>(null);
  let loadError = $state<string | null>(null);
  let busy = $state<string | null>(null);

  // Back up -----------------------------------------------------------------
  let ticks = $state<Record<string, boolean>>({});
  let contents = $state<Contents | null>(null);
  let contentsError = $state<string | null>(null);
  let backupMsg = $state<Msg | null>(null);
  let contentsSeq = 0;

  const backupIds = $derived(overview ? chosen(overview.categories, ticks) : []);

  // Restore -----------------------------------------------------------------
  let inspected = $state<Inspected | null>(null);
  let rticks = $state<Record<string, boolean>>({});
  let mode = $state<Mode>("merge");
  let shown = $state<RestorePreview | null>(null);
  let previewing = $state(false);
  let previewError = $state<string | null>(null);
  let restoreMsg = $state<Msg | null>(null);
  let result = $state<RestoreResult | null>(null);
  let confirming = $state(false);
  let previewSeq = 0;

  const restoreIds = $derived(inspected ? chosen(inspected.categories, rticks) : []);
  const willChange = $derived(changesSomething(shown));
  const willRemove = $derived(removals(shown));
  const canRestore = $derived(
    !!inspected &&
      restoreIds.length > 0 &&
      !!shown &&
      !shown.problem &&
      !previewError &&
      !previewing &&
      willChange &&
      busy === null,
  );

  // Automatic backups -------------------------------------------------------
  let auto = $state<AutoSettings>({
    schedule: "off",
    on_update: false,
    keep: 5,
    folder: "",
    resolved_folder: "",
  });
  let autoSaved = $state("");
  let autoMsg = $state<Msg | null>(null);
  const autoDirty = $derived(JSON.stringify(auto) !== autoSaved);

  async function refresh() {
    const loaded = await getOverview();
    if (!loaded.ok) {
      loadError = loaded.error;
      return;
    }
    loadError = null;
    overview = loaded.value;
    if (Object.keys(ticks).length === 0) ticks = defaultTicks(loaded.value.categories);
    auto = { ...loaded.value.auto };
    autoSaved = JSON.stringify(auto);
  }

  onMount(() => {
    void refresh();
  });

  // What a backup of the ticked categories would hold.
  $effect(() => {
    const ids = backupIds;
    if (!overview) return;
    const seq = ++contentsSeq;
    if (ids.length === 0) {
      contents = null;
      contentsError = null;
      return;
    }
    void getContents(ids).then((got) => {
      if (seq !== contentsSeq) return;
      if (got.ok) {
        contents = got.value;
        contentsError = null;
      } else {
        contents = null;
        contentsError = got.error;
      }
    });
  });

  // What the restore would change, for the ticked categories and the mode.
  $effect(() => {
    const ids = restoreIds;
    const chosenMode = mode;
    const file = inspected;
    if (!file) return;
    const seq = ++previewSeq;
    confirming = false;
    if (ids.length === 0) {
      shown = null;
      previewError = null;
      return;
    }
    previewing = true;
    void previewRestore(ids, chosenMode).then((got) => {
      if (seq !== previewSeq) return;
      previewing = false;
      if (got.ok) {
        shown = got.value;
        previewError = null;
      } else {
        shown = null;
        previewError = got.error;
      }
    });
  });

  async function save() {
    busy = "save";
    backupMsg = null;
    const done = await saveBackupAs(backupIds);
    busy = null;
    if (!done.ok) backupMsg = { kind: "error", text: done.error };
    else if (done.value) {
      backupMsg = { kind: "ok", text: `Saved to ${done.value.display} (${formatBytes(done.value.bytes)}).` };
      await refresh();
    }
  }

  async function now() {
    busy = "now";
    backupMsg = null;
    const done = await backupNow(backupIds);
    busy = null;
    if (!done.ok) backupMsg = { kind: "error", text: done.error };
    else {
      backupMsg = { kind: "ok", text: `Saved to ${done.value.display} (${formatBytes(done.value.bytes)}).` };
      await refresh();
    }
  }

  async function openFolder() {
    const done = await openBackupFolder();
    if (!done.ok) backupMsg = { kind: "error", text: done.error };
  }

  async function choose() {
    busy = "pick";
    restoreMsg = null;
    const picked = await pickBackup();
    busy = null;
    if (!picked.ok) {
      restoreMsg = { kind: "error", text: picked.error };
      return;
    }
    if (!picked.value) return;
    result = null;
    shown = null;
    previewError = null;
    mode = "merge";
    rticks = Object.fromEntries(picked.value.categories.map((c) => [c.id, true]));
    inspected = picked.value;
  }

  function cancelRestore() {
    inspected = null;
    shown = null;
    previewError = null;
    confirming = false;
    restoreMsg = null;
  }

  function askOrRestore() {
    // Replace removes things; ask once, in the page, before it does.
    if (mode === "replace" && willRemove > 0 && !confirming) {
      confirming = true;
      return;
    }
    void restoreNow();
  }

  async function restoreNow() {
    if (!inspected) return;
    confirming = false;
    busy = "restore";
    restoreMsg = null;
    const done = await applyRestore(restoreIds, mode, inspected.archive.sha256);
    busy = null;
    if (!done.ok) {
      restoreMsg = { kind: "error", text: done.error };
      return;
    }
    result = done.value;
    inspected = null;
    shown = null;
    restoreMsg = {
      kind: "ok",
      text: done.value.changed
        ? "Restored. The new settings are already in use."
        : "Nothing needed to change.",
    };
    await refresh();
    if (done.value.changed) await onrestored?.();
  }

  async function undo() {
    busy = "undo";
    restoreMsg = null;
    const done = await undoRestore();
    busy = null;
    if (!done.ok) {
      restoreMsg = { kind: "error", text: done.error };
      return;
    }
    result = null;
    restoreMsg = { kind: "ok", text: "Undone. What the restore replaced is back." };
    await refresh();
    await onrestored?.();
  }

  async function saveAuto() {
    busy = "auto";
    autoMsg = null;
    auto.keep = clampKeep(auto.keep);
    const done = await setAuto(auto);
    busy = null;
    if (!done.ok) {
      autoMsg = { kind: "error", text: done.error };
      return;
    }
    auto = { ...done.value };
    autoSaved = JSON.stringify(auto);
    autoMsg = { kind: "ok", text: "Saved." };
    await refresh();
  }

  const kindWord = (kind: string) => (kind === "auto" ? "automatic backup" : "backup");
</script>

<div class="bk-page">
  <h1>Backup &amp; restore</h1>
  <p class="bk-lead">
    Save your settings, snippets, web searches, themes, script plugins and workflows to one file,
    and put them back on this or another computer. Nothing is sent anywhere.
  </p>

  {#if loadError}
    <p class="bk-msg error" role="alert">Could not load this page: {loadError}</p>
  {:else if !overview}
    <p class="bk-muted">Loading…</p>
  {:else}
    <!-- Back up ------------------------------------------------------------>
    <section class="bk-card" aria-labelledby="bk-backup-h">
      <h2 id="bk-backup-h">Back up</h2>

      <fieldset class="bk-fieldset">
        <legend>What to include</legend>
        {#each overview.categories as category (category.id)}
          {@const listed = contents?.categories.find((c) => c.category === category.id)}
          <div class="bk-choice">
            <input
              id="bk-cat-{category.id}"
              type="checkbox"
              bind:checked={ticks[category.id]}
              disabled={busy !== null}
            />
            <label for="bk-cat-{category.id}">
              <span class="bk-name">{category.label}</span>
              <span class="bk-hint">{category.description}</span>
            </label>
            {#if listed && ticks[category.id]}
              <span class="bk-count">{listed.files} file{listed.files === 1 ? "" : "s"}</span>
            {/if}
          </div>
        {/each}
      </fieldset>

      {#if contentsError}
        <p class="bk-msg error" role="alert">{contentsError}</p>
      {/if}

      {#if contents}
        <details class="bk-details">
          <summary>Show exactly what is included ({formatBytes(contents.bytes)})</summary>
          {#each contents.categories as listed (listed.category)}
            <p class="bk-sub"><strong>{listed.label}</strong></p>
            {#if listed.entries.length === 0}
              <p class="bk-muted">Nothing yet.</p>
            {:else}
              <ul class="bk-list">
                {#each listed.entries as entry (entry)}
                  <li>{entry}</li>
                {/each}
              </ul>
            {/if}
          {/each}
          {#each contents.warnings as warning (warning)}
            <p class="bk-msg warn">{warning}</p>
          {/each}
          {#if contents.left_out.length > 0}
            <p class="bk-muted">
              Left out of config.toml on purpose: {contents.left_out.join(", ")}.
            </p>
          {/if}
        </details>
      {/if}

      <div class="bk-note privacy">
        <strong>Never included:</strong>
        <ul class="bk-list">
          {#each overview.never_included as line (line)}
            <li>{line}</li>
          {/each}
        </ul>
        <p>
          The backup is a plain file, <strong>not encrypted</strong>: anyone who can open it can
          read your snippets and scripts. Sevak makes it readable by you only; keep it somewhere
          you trust.
        </p>
      </div>

      <div class="bk-actions">
        <button
          type="button"
          class="bk-btn primary"
          disabled={busy !== null || backupIds.length === 0}
          onclick={save}
        >
          {busy === "save" ? "Saving…" : "Save backup as…"}
        </button>
        <button
          type="button"
          class="bk-btn"
          disabled={busy !== null || backupIds.length === 0}
          onclick={now}
        >
          {busy === "now" ? "Backing up…" : "Back up now"}
        </button>
        <button type="button" class="bk-btn" onclick={openFolder}>Open backup folder</button>
      </div>
      <p class="bk-hint">“Back up now” saves to <code>{overview.folder}</code> without asking.</p>

      {#if backupMsg}
        <p class="bk-msg {backupMsg.kind}" role={backupMsg.kind === "error" ? "alert" : "status"}>
          {backupMsg.text}
        </p>
      {/if}
      {#if overview.last_backup}
        <p class="bk-muted">
          Last {kindWord(overview.last_backup.kind)}: {formatWhen(overview.last_backup.created)},
          <code>{overview.last_backup.path}</code>{overview.last_backup.exists ? "" : " (the file is no longer there)"}
        </p>
      {:else}
        <p class="bk-muted">No backup made yet.</p>
      {/if}
    </section>

    <!-- Restore ------------------------------------------------------------>
    <section class="bk-card" aria-labelledby="bk-restore-h">
      <h2 id="bk-restore-h">Restore</h2>
      <p class="bk-sub">
        Choose a backup file. Sevak checks it and shows what would change before it changes
        anything. A safety copy of what is replaced is saved first, so you can undo.
      </p>

      <div class="bk-actions">
        <button type="button" class="bk-btn primary" disabled={busy !== null} onclick={choose}>
          {busy === "pick" ? "Checking…" : inspected ? "Choose another backup…" : "Choose a backup…"}
        </button>
        {#if inspected}
          <button type="button" class="bk-btn" disabled={busy !== null} onclick={cancelRestore}>
            Cancel
          </button>
        {/if}
      </div>

      {#if restoreMsg}
        <p class="bk-msg {restoreMsg.kind}" role={restoreMsg.kind === "error" ? "alert" : "status"}>
          {restoreMsg.text}
        </p>
      {/if}

      {#if inspected}
        <div class="bk-file">
          <p class="bk-name"><code>{inspected.display}</code></p>
          <p class="bk-muted">
            Made {formatWhen(inspected.archive.created)} by Sevak {inspected.archive.app_version} on
            {platformName(inspected.archive.platform)} · {inspected.archive.files}
            file{inspected.archive.files === 1 ? "" : "s"} · {formatBytes(inspected.archive.size)}
          </p>
          {#each inspected.warnings as warning (warning)}
            <p class="bk-msg warn">{warning}</p>
          {/each}
        </div>

        <fieldset class="bk-fieldset">
          <legend>What to restore</legend>
          {#each inspected.categories as category (category.id)}
            {@const line = shown?.categories.find((c) => c.category === category.id)}
            <div class="bk-choice">
              <input
                id="bk-rcat-{category.id}"
                type="checkbox"
                bind:checked={rticks[category.id]}
                disabled={busy !== null}
              />
              <label for="bk-rcat-{category.id}">
                <span class="bk-name">{category.label}</span>
                {#if line && rticks[category.id]}
                  <span class="bk-hint">{summarize(line)}</span>
                {/if}
              </label>
            </div>
          {/each}
        </fieldset>

        <fieldset class="bk-fieldset">
          <legend>How</legend>
          <div class="bk-choice">
            <input id="bk-mode-merge" type="radio" name="bk-mode" value="merge" bind:group={mode} disabled={busy !== null} />
            <label for="bk-mode-merge">
              <span class="bk-name">Merge</span>
              <span class="bk-hint">
                Adds what the backup has and overwrites what has the same name or setting. Keeps
                everything else you have.
              </span>
            </label>
          </div>
          <div class="bk-choice">
            <input id="bk-mode-replace" type="radio" name="bk-mode" value="replace" bind:group={mode} disabled={busy !== null} />
            <label for="bk-mode-replace">
              <span class="bk-name">Replace</span>
              <span class="bk-hint">
                Makes the chosen categories exactly like the backup: what the backup does not have
                is removed, or goes back to its default.
              </span>
            </label>
          </div>
        </fieldset>

        {#if previewing}
          <p class="bk-muted" role="status">Checking what would change…</p>
        {/if}
        {#if previewError}
          <p class="bk-msg error" role="alert">{previewError}</p>
        {/if}
        {#if shown}
          {#if shown.problem}
            <p class="bk-msg error" role="alert">{shown.problem}</p>
          {/if}
          {#each shown.warnings as warning (warning)}
            <p class="bk-msg warn">{warning}</p>
          {/each}

          <div class="bk-preview" aria-live="polite">
            <h3>What would change</h3>
            {#each shown.categories.filter((c) => c.selected) as category (category.category)}
              <details class="bk-details" open={category.added + category.changed + category.removed > 0 && category.items.length <= 12}>
                <summary>
                  <span class="bk-name">{category.label}</span>: {summarize(category)}
                </summary>
                {#if category.problem}
                  <p class="bk-msg error">{category.problem}</p>
                {:else if category.items.length === 0}
                  <p class="bk-muted">Nothing in the backup for this.</p>
                {:else}
                  <ul class="bk-items">
                    {#each category.items as item, i (i)}
                      <li>
                        <span class="bk-badge {item.change}">{changeLabel(item.change, shown.mode)}</span>
                        <span class="bk-item-name">{item.name}</span>
                        {#if item.needs_approval}
                          <span class="bk-flag">asks for approval</span>
                        {/if}
                        {#if item.files.length > 0 && item.change !== "unchanged"}
                          <span class="bk-files">{item.files.join(", ")}</span>
                        {/if}
                      </li>
                    {/each}
                  </ul>
                {/if}
              </details>
            {/each}

            {#if shown.needs_approval.length > 0}
              <p class="bk-note">
                <strong>Scripts are never trusted automatically.</strong> After the restore Sevak
                asks before it runs {shown.needs_approval.join(", ")}. Which scripts you allowed is
                not part of a backup.
              </p>
            {/if}
            {#if !willChange && !shown.problem}
              <p class="bk-muted">Everything chosen is already the same: there is nothing to restore.</p>
            {/if}
          </div>

          {#if confirming}
            <div class="bk-note warn" role="alert">
              <strong>Replace will remove {willRemove} thing{willRemove === 1 ? "" : "s"}</strong>
              that {willRemove === 1 ? "is" : "are"} not in the backup (or reset
              {willRemove === 1 ? "it" : "them"} to the default). A safety copy is saved first, and
              “Undo restore” puts it back.
              <div class="bk-actions">
                <button type="button" class="bk-btn primary" disabled={busy !== null} onclick={restoreNow}>
                  Replace now
                </button>
                <button type="button" class="bk-btn" onclick={() => (confirming = false)}>Cancel</button>
              </div>
            </div>
          {:else}
            <div class="bk-actions">
              <button type="button" class="bk-btn primary" disabled={!canRestore} onclick={askOrRestore}>
                {busy === "restore" ? "Restoring…" : mode === "replace" ? "Replace…" : "Restore"}
              </button>
            </div>
          {/if}
        {/if}
      {/if}

      {#if result && result.changed}
        <div class="bk-note" role="status">
          <strong>Restored.</strong>
          {#each result.categories as category (category.category)}
            <span class="bk-chip">{category.label}: {summarize(category)}</span>
          {/each}
          {#if result.needs_approval.length > 0}
            <p>Sevak will ask before it runs {result.needs_approval.join(", ")}.</p>
          {/if}
          {#each result.warnings as warning (warning)}
            <p class="bk-msg warn">{warning}</p>
          {/each}
        </div>
      {/if}

      {#if overview.undo}
        <div class="bk-undo">
          <p class="bk-sub">
            The last restore ({formatWhen(overview.undo.created)}) can be undone. It puts back
            what was replaced: {overview.undo.categories.join(", ").replace(/_/g, " ")}.
          </p>
          <button type="button" class="bk-btn" disabled={busy !== null} onclick={undo}>
            {busy === "undo" ? "Undoing…" : "Undo restore"}
          </button>
        </div>
      {/if}
    </section>

    <!-- Automatic backups -------------------------------------------------->
    <section class="bk-card" aria-labelledby="bk-auto-h">
      <h2 id="bk-auto-h">Automatic backups</h2>
      <p class="bk-sub">
        Off unless you turn it on. Sevak keeps the newest few and deletes older automatic backups
        only; it never touches files you saved yourself.
      </p>
      {#if overview.auto_problem}
        <p class="bk-msg error" role="alert">{overview.auto_problem}</p>
      {/if}

      <div class="bk-row">
        <label for="bk-schedule" class="bk-name">Back up</label>
        <select id="bk-schedule" class="bk-input" bind:value={auto.schedule} disabled={busy !== null}>
          {#each SCHEDULES as option (option.value)}
            <option value={option.value}>{option.label}</option>
          {/each}
        </select>
      </div>

      <div class="bk-row">
        <span class="bk-name" id="bk-update-label">Also when Sevak is updated</span>
        <Toggle bind:checked={auto.on_update} label="Also back up when Sevak is updated" disabled={busy !== null} />
      </div>

      <div class="bk-row">
        <label for="bk-keep" class="bk-name">Keep the newest</label>
        <input
          id="bk-keep"
          class="bk-input number"
          type="number"
          min="1"
          max="50"
          step="1"
          bind:value={auto.keep}
          disabled={busy !== null}
        />
      </div>

      <div class="bk-field">
        <label for="bk-folder" class="bk-name">Folder</label>
        <input
          id="bk-folder"
          class="bk-input wide"
          type="text"
          spellcheck="false"
          autocomplete="off"
          placeholder={auto.resolved_folder || "Documents/Sevak backups"}
          bind:value={auto.folder}
          disabled={busy !== null}
        />
        <p class="bk-hint">
          Used by automatic backups and “Back up now”. Leave empty for
          <code>{overview.auto.resolved_folder || "Documents/Sevak backups"}</code>. A full path, or
          one starting with <code>~</code>.
        </p>
      </div>

      <div class="bk-actions">
        <button type="button" class="bk-btn primary" disabled={!autoDirty || busy !== null} onclick={saveAuto}>
          {busy === "auto" ? "Saving…" : "Save"}
        </button>
        {#if autoMsg}
          <span class="bk-msg {autoMsg.kind}" role={autoMsg.kind === "error" ? "alert" : "status"}>
            {autoMsg.text}
          </span>
        {/if}
      </div>
      <p class="bk-hint">
        The same options are in <code>backup.toml</code> next to <code>config.toml</code>. From a
        terminal: <code>sevak --backup &lt;file&gt;</code>, <code>sevak --restore &lt;file&gt;</code>,
        <code>sevak --undo-restore</code>. In the launcher: <code>backup settings</code>,
        <code>restore settings</code>.
      </p>
    </section>
  {/if}
</div>

<style>
  .bk-page h1 {
    margin: 0 0 6px;
    font-size: 19px;
    font-weight: 650;
  }

  .bk-lead {
    margin: 0 0 14px;
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.5;
  }

  .bk-card {
    margin: 0 0 16px;
    padding: 14px 16px 16px;
    border: 1px solid var(--border);
    border-radius: 11px;
    background: var(--surface);
  }

  .bk-card h2 {
    margin: 0 0 8px;
    font-size: 14px;
    font-weight: 650;
  }

  .bk-card h3 {
    margin: 12px 0 4px;
    font-size: 13px;
    font-weight: 650;
  }

  .bk-sub,
  .bk-muted,
  .bk-hint {
    margin: 0 0 8px;
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.5;
  }

  .bk-hint {
    display: block;
    font-size: 12px;
  }

  .bk-sub strong {
    color: var(--fg);
  }

  code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .bk-fieldset {
    margin: 0 0 10px;
    padding: 0;
    border: 0;
    min-width: 0;
  }

  .bk-fieldset legend {
    padding: 0;
    margin-bottom: 4px;
    font-size: 12px;
    font-weight: 600;
  }

  .bk-choice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 5px 0;
  }

  .bk-choice input {
    flex: none;
    margin: 3px 0 0;
    width: 16px;
    height: 16px;
    accent-color: var(--accent-strong);
  }

  .bk-choice label {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    cursor: pointer;
  }

  .bk-choice input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .bk-count {
    flex: none;
    color: var(--muted);
    font-size: 12px;
    white-space: nowrap;
  }

  .bk-name {
    font-weight: 560;
  }

  .bk-details {
    margin: 6px 0;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
  }

  .bk-details summary {
    cursor: pointer;
    font-size: 12.5px;
  }

  .bk-details summary:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 4px;
  }

  .bk-list {
    margin: 2px 0 6px;
    padding-left: 18px;
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.5;
  }

  .bk-note {
    margin: 10px 0;
    padding: 9px 12px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--bg);
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.5;
  }

  .bk-note strong {
    color: var(--fg);
  }

  .bk-note p {
    margin: 6px 0 0;
  }

  .bk-note.privacy {
    border-left: 3px solid var(--accent-strong);
  }

  .bk-note.warn {
    border-color: var(--warn);
    color: var(--fg);
  }

  .bk-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 10px 0 6px;
  }

  .bk-btn {
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

  .bk-btn:hover:not(:disabled) {
    border-color: var(--accent-strong);
  }

  .bk-btn:focus-visible,
  .bk-input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .bk-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .bk-btn.primary {
    border-color: var(--accent-strong);
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
  }

  .bk-btn.primary:hover:not(:disabled) {
    background: var(--accent-strong);
  }

  .bk-msg {
    margin: 0 0 8px;
    font-size: 12.5px;
    line-height: 1.4;
  }

  .bk-msg.ok {
    color: var(--ok);
  }

  .bk-msg.error {
    color: var(--error);
  }

  .bk-msg.warn {
    color: var(--warn);
  }

  .bk-file {
    margin: 10px 0;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--bg);
  }

  .bk-file p {
    margin: 0 0 4px;
  }

  .bk-items {
    list-style: none;
    margin: 6px 0 2px;
    padding: 0;
    font-size: 12.5px;
  }

  .bk-items li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 8px;
    padding: 2px 0;
  }

  .bk-badge {
    flex: none;
    min-width: 58px;
    padding: 0 6px;
    border-radius: 9px;
    background: var(--kbd-bg);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-align: center;
  }

  .bk-badge.added {
    color: var(--ok);
  }

  .bk-badge.changed {
    color: var(--warn);
  }

  .bk-badge.removed {
    color: var(--error);
  }

  .bk-flag {
    color: var(--warn);
    font-size: 11.5px;
  }

  .bk-files {
    color: var(--muted);
    font-size: 11.5px;
    overflow-wrap: anywhere;
  }

  .bk-chip {
    display: inline-block;
    margin: 2px 8px 0 0;
    color: var(--muted);
  }

  .bk-undo {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }

  .bk-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 7px 0;
  }

  .bk-field {
    padding: 7px 0;
  }

  .bk-field .bk-input {
    margin: 4px 0;
  }

  .bk-input {
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
  }

  .bk-input.number {
    width: 90px;
  }

  .bk-input.wide {
    width: min(100%, 420px);
    box-sizing: border-box;
  }

  .bk-input:disabled {
    opacity: 0.5;
  }
</style>
