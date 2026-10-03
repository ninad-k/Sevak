<script lang="ts">
  import { onMount, tick } from "svelte";
  import HotkeyField from "./lib/HotkeyField.svelte";
  import Toggle from "./lib/Toggle.svelte";
  import { getStatus, onStatus, type Status } from "./lib/ipc";
  import {
    closeSettings,
    getSettings,
    openConfigFile,
    openLogDir,
    pickDirectory,
    saveSettings,
    setupWaylandHotkey,
    validateHotkey,
    type Config,
    type Outcome,
    type PluginInfo,
    type SettingsDto,
  } from "./lib/settings-ipc";
  import { applyTheme } from "./lib/theme";
  import {
    MAX_DEPTH,
    MAX_WIDTH,
    MIN_WIDTH,
    totalProblems,
    validate,
    type SectionId,
  } from "./lib/validate";

  const emptyConfig = (): Config => ({
    general: { hotkey: "", hide_on_blur: true, launch_at_login: false, check_for_updates: true },
    window: { width: 720 },
    linux: { wayland_use_xwayland: true },
    search: { max_results: 8, fallback_web_search: "" },
    appearance: { theme: "system" },
    plugins: { disabled: [] },
    calculator: { currency: false },
    files: { directories: [], max_depth: 4, include_hidden: false, keyword: "", global: true },
    web_search: [],
  });

  const clone = <T,>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

  let loaded = $state<SettingsDto | null>(null);
  let loadFailed = $state(false);
  let draft = $state<Config>(emptyConfig());
  /** The config as last loaded from (or saved to) disk, to tell whether the form is dirty. */
  let baseline = $state("");
  /** `linux.wayland_use_xwayland` as it was when this window opened. */
  let xwaylandAtOpen = $state<boolean | null>(null);
  let status = $state<Status | null>(null);

  let active = $state<SectionId>("general");
  let hotkeyError = $state<string | null>(null);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let toastVisible = $state(false);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let footerError = $state<string | null>(null);
  let waylandBusy = $state(false);
  let waylandResult = $state<Outcome | null>(null);
  let content: HTMLElement | undefined = $state();

  const ready = $derived(loaded !== null);
  const display = $derived(loaded?.display ?? "unknown");
  const isWayland = $derived(display === "wayland");
  const showLinux = $derived(display === "x11" || display === "wayland");
  const dirty = $derived(ready && JSON.stringify(draft) !== baseline);
  const problems = $derived(validate(draft));
  const problemCount = $derived(totalProblems(problems) + (hotkeyError ? 1 : 0));
  const canSave = $derived(ready && dirty && problemCount === 0 && !saving);

  const sections = $derived(
    (
      [
        { id: "general", label: "General" },
        { id: "appearance", label: "Appearance" },
        { id: "search", label: "Search" },
        { id: "plugins", label: "Plugins" },
        { id: "web", label: "Web search" },
        { id: "files", label: "Files" },
        ...(showLinux ? [{ id: "linux", label: "Linux" }] : []),
      ] as { id: SectionId; label: string }[]
    ).map((section) => ({
      ...section,
      problems: problems.count[section.id] + (section.id === "general" && hotkeyError ? 1 : 0),
    })),
  );

  const themes = [
    { value: "system", label: "System" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ] as const;

  const engineKeywords = $derived(
    [...new Set(draft.web_search.map((e) => e.keyword.trim()).filter((k) => k !== ""))].map(
      (keyword) => ({
        keyword,
        name: draft.web_search.find((e) => e.keyword.trim() === keyword)?.name.trim() ?? "",
      }),
    ),
  );
  const fallbackMissing = $derived(
    draft.search.fallback_web_search !== "" &&
      !engineKeywords.some((e) => e.keyword === draft.search.fallback_web_search),
  );

  /** Plugin rows: the backend catalog, with the web engines taken live from the form. */
  const pluginRows = $derived.by(() => {
    const engines: PluginInfo[] = draft.web_search
      .filter((engine) => engine.keyword.trim() !== "")
      .map((engine) => {
        const keyword = engine.keyword.trim();
        const name = engine.name.trim() || keyword;
        return {
          id: `web:${keyword}`,
          name,
          description: `Search ${name} by typing \`${keyword} <terms>\`.`,
          keyword,
          enabled: true,
        };
      });
    const rows: PluginInfo[] = [];
    let placedWeb = false;
    for (const info of loaded?.catalog ?? []) {
      if (info.id.startsWith("web:")) {
        if (!placedWeb) rows.push(...engines);
        placedWeb = true;
      } else if (info.id === "files") {
        rows.push({ ...info, keyword: draft.files.keyword.trim() || null });
      } else {
        rows.push(info);
      }
    }
    if (!placedWeb) rows.push(...engines);
    return rows;
  });

  const family = (id: string) => id.split(":")[0];
  const pluginEnabled = (id: string) =>
    !draft.plugins.disabled.includes(id) && !draft.plugins.disabled.includes(family(id));

  function setPlugin(id: string, on: boolean) {
    const disabled = draft.plugins.disabled;
    if (!on) {
      if (!disabled.includes(id)) draft.plugins.disabled = [...disabled, id];
      return;
    }
    let next = disabled.filter((entry) => entry !== id);
    const fam = family(id);
    if (fam !== id && next.includes(fam)) {
      // The whole family was off: switching one engine on keeps its siblings off.
      next = next.filter((entry) => entry !== fam);
      for (const row of pluginRows) {
        if (row.id !== id && family(row.id) === fam) next.push(row.id);
      }
    }
    draft.plugins.disabled = next;
  }

  // The shortcut is parsed by Rust (same parser as the global-shortcut plugin).
  let hotkeySeq = 0;
  $effect(() => {
    const value = draft.general.hotkey;
    if (!ready) return;
    const mine = ++hotkeySeq;
    const timer = setTimeout(async () => {
      const error = await validateHotkey(value);
      if (mine === hotkeySeq) hotkeyError = error;
    }, 150);
    return () => clearTimeout(timer);
  });

  // Preview the theme while choosing it; saving makes it stick for every window.
  $effect(() => {
    if (ready) applyTheme(draft.appearance.theme);
  });

  const registrationWarning = $derived(
    !isWayland &&
      status?.hotkey.error &&
      status.hotkey.accelerator === draft.general.hotkey.trim()
      ? status.hotkey.error
      : null,
  );

  async function load() {
    const dto = await getSettings();
    if (!dto) {
      loadFailed = true;
      return;
    }
    loaded = dto;
    draft = clone(dto.config);
    baseline = JSON.stringify(dto.config);
    xwaylandAtOpen ??= dto.config.linux.wayland_use_xwayland;
  }

  function showToast() {
    toastVisible = true;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toastVisible = false), 2200);
  }

  async function save() {
    if (!canSave) return;
    saving = true;
    saveError = null;
    const payload = clone($state.snapshot(draft)) as Config;
    payload.general.hotkey = payload.general.hotkey.trim();
    payload.files.keyword = payload.files.keyword.trim();
    payload.files.directories = payload.files.directories.map((dir) => dir.trim());
    payload.search.fallback_web_search = payload.search.fallback_web_search.trim();
    payload.web_search = payload.web_search.map((engine) => ({
      keyword: engine.keyword.trim(),
      name: engine.name.trim(),
      url: engine.url.trim(),
    }));
    const error = await saveSettings(payload);
    saving = false;
    if (error) {
      saveError = error;
      return;
    }
    await load();
    showToast();
  }

  async function cancel() {
    const error = await closeSettings();
    if (error) footerError = error;
  }

  async function runFooter(action: () => Promise<string | null>) {
    footerError = (await action()) ?? null;
  }

  async function setUpWayland() {
    waylandBusy = true;
    waylandResult = null;
    waylandResult = await setupWaylandHotkey(draft.general.hotkey);
    waylandBusy = false;
  }

  async function addDirectory() {
    const picked = await pickDirectory();
    if (picked && !draft.files.directories.includes(picked)) {
      draft.files.directories = [...draft.files.directories, picked];
    }
  }

  function removeDirectory(index: number) {
    draft.files.directories = draft.files.directories.filter((_, i) => i !== index);
  }

  async function addEngine() {
    draft.web_search = [...draft.web_search, { keyword: "", name: "", url: "https://" }];
    await tick();
    const rows = content?.querySelectorAll<HTMLInputElement>("[data-engine] input[data-field='keyword']");
    rows?.[rows.length - 1]?.focus();
  }

  function removeEngine(index: number) {
    const removed = draft.web_search[index];
    draft.web_search = draft.web_search.filter((_, i) => i !== index);
    const stillDefined = draft.web_search.some((e) => e.keyword.trim() === removed.keyword.trim());
    if (!stillDefined && draft.search.fallback_web_search === removed.keyword.trim()) {
      draft.search.fallback_web_search = "";
    }
  }

  function renameKeyword(index: number, value: string) {
    const old = draft.web_search[index].keyword;
    draft.web_search[index].keyword = value;
    // The fallback follows a renamed engine instead of silently dangling.
    if (old.trim() !== "" && draft.search.fallback_web_search === old.trim()) {
      draft.search.fallback_web_search = value.trim();
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save();
    } else if (
      import.meta.env.PROD &&
      (e.key === "F5" || ((e.ctrlKey || e.metaKey) && ["r", "f", "p"].includes(e.key.toLowerCase())))
    ) {
      e.preventDefault();
    }
  }

  function onTabKeydown(e: KeyboardEvent, index: number) {
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    const target =
      e.key === "Home" ? 0 : e.key === "End" ? sections.length - 1 : index + step;
    if (target === index || target < 0 || target >= sections.length) {
      if (step !== 0 || e.key === "Home" || e.key === "End") e.preventDefault();
      return;
    }
    e.preventDefault();
    active = sections[target].id;
    void tick().then(() => document.getElementById(`tab-${active}`)?.focus());
  }

  function onContextMenu(e: MouseEvent) {
    if (import.meta.env.PROD) e.preventDefault();
  }

  onMount(() => {
    void load();
    void getStatus().then((s) => (status = s));
    const unlisten = onStatus((s) => (status = s));
    return () => {
      void unlisten.then((fn) => fn());
      clearTimeout(toastTimer);
    };
  });

  // Keep the pane on top when switching sections.
  $effect(() => {
    void active;
    if (content) content.scrollTop = 0;
  });
</script>

<svelte:window onkeydown={onKeydown} oncontextmenu={onContextMenu} />
<svelte:head>
  <title>Sevak Settings</title>
</svelte:head>

<div class="app">
  {#if loadFailed}
    <div class="state" role="alert">
      Could not load the settings. Close this window and try again from the tray menu.
    </div>
  {:else if !ready}
    <div class="state" role="status">Loading…</div>
  {:else}
    <div class="body">
      <div class="sidebar" role="tablist" aria-orientation="vertical" aria-label="Settings sections">
        {#each sections as section, i (section.id)}
          <button
            id="tab-{section.id}"
            type="button"
            class="tab"
            class:active={active === section.id}
            role="tab"
            aria-selected={active === section.id}
            aria-controls="panel"
            tabindex={active === section.id ? 0 : -1}
            onclick={() => (active = section.id)}
            onkeydown={(e) => onTabKeydown(e, i)}
          >
            <span>{section.label}</span>
            {#if section.problems > 0}
              <span class="dot" title="{section.problems} problem(s)" aria-label="{section.problems} problem(s)"></span>
            {/if}
          </button>
        {/each}
      </div>

      <div id="panel" class="content" role="tabpanel" aria-labelledby="tab-{active}" bind:this={content}>
        {#if active === "general"}
          <h1>General</h1>

          <section class="group">
            <div class="field">
              <label class="name" for="hotkey">Shortcut</label>
              {#if isWayland}
                <p class="hint">
                  Wayland: the desktop owns global shortcuts. Bind this key to
                  <code>sevak --toggle</code> in your desktop settings, or let Sevak do it on GNOME.
                </p>
              {:else}
                <p class="hint">Shows and hides Sevak from anywhere.</p>
              {/if}
              <HotkeyField id="hotkey" bind:value={draft.general.hotkey} error={hotkeyError} />
              {#if registrationWarning}
                <p class="msg warn" role="status">
                  This shortcut could not be registered: {registrationWarning}
                </p>
              {/if}
              {#if isWayland}
                <div class="inline">
                  <button
                    type="button"
                    class="btn"
                    disabled={waylandBusy || !!hotkeyError}
                    onclick={setUpWayland}
                  >
                    {waylandBusy ? "Setting up…" : "Set up GNOME shortcut"}
                  </button>
                </div>
                {#if waylandResult}
                  <pre class="result" class:fail={!waylandResult.ok} role="status">{waylandResult.text}</pre>
                {/if}
              {/if}
            </div>

            <div class="row">
              <div class="label">
                <span class="name">Hide when focus is lost</span>
                <span class="hint">Closes the search bar when you click elsewhere.</span>
              </div>
              <Toggle bind:checked={draft.general.hide_on_blur} label="Hide when focus is lost" />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">Launch at login</span>
                <span class="hint">Starts Sevak in the background when you sign in.</span>
              </div>
              <Toggle bind:checked={draft.general.launch_at_login} label="Launch at login" />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">Check for updates</span>
                <span class="hint">Looks for a new version at startup and daily, and asks before installing.</span>
              </div>
              <Toggle bind:checked={draft.general.check_for_updates} label="Check for updates" />
            </div>
          </section>
        {:else if active === "appearance"}
          <h1>Appearance</h1>

          <section class="group">
            <div class="row">
              <div class="label">
                <span class="name" id="theme-label">Theme</span>
                <span class="hint">System follows your operating system.</span>
              </div>
              <div class="segmented" role="radiogroup" aria-labelledby="theme-label">
                {#each themes as theme (theme.value)}
                  <label class:checked={draft.appearance.theme === theme.value}>
                    <input
                      type="radio"
                      name="theme"
                      value={theme.value}
                      bind:group={draft.appearance.theme}
                    />
                    <span>{theme.label}</span>
                  </label>
                {/each}
              </div>
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="width">Window width</label>
                <span class="hint">Width of the search bar.</span>
              </div>
              <div class="slider">
                <input
                  id="width"
                  type="range"
                  min={MIN_WIDTH}
                  max={MAX_WIDTH}
                  step="10"
                  bind:value={draft.window.width}
                />
                <output for="width">{draft.window.width} px</output>
              </div>
            </div>
          </section>
        {:else if active === "search"}
          <h1>Search</h1>

          <section class="group">
            <div class="row">
              <div class="label">
                <label class="name" for="max-results">Maximum results</label>
                <span class="hint">How many rows the list shows.</span>
              </div>
              <div class="slider">
                <input
                  id="max-results"
                  type="range"
                  min="1"
                  max="20"
                  step="1"
                  bind:value={draft.search.max_results}
                />
                <output for="max-results">{draft.search.max_results}</output>
              </div>
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="fallback">Fallback web search</label>
                <span class="hint">Offered when nothing else matches your query.</span>
                {#if problems.fallback}
                  <span class="msg error" role="alert">{problems.fallback}</span>
                {/if}
              </div>
              <select
                id="fallback"
                class="input select"
                class:invalid={!!problems.fallback}
                bind:value={draft.search.fallback_web_search}
              >
                <option value="">None</option>
                {#each engineKeywords as engine (engine.keyword)}
                  <option value={engine.keyword}>
                    {engine.name ? `${engine.name} (${engine.keyword})` : engine.keyword}
                  </option>
                {/each}
                {#if fallbackMissing}
                  <option value={draft.search.fallback_web_search}>
                    {draft.search.fallback_web_search} (missing)
                  </option>
                {/if}
              </select>
            </div>
          </section>
        {:else if active === "plugins"}
          <h1>Plugins</h1>

          <section class="group">
            {#each pluginRows as plugin (plugin.id)}
              <div class="row">
                <div class="label">
                  <span class="name">
                    {plugin.name}
                    {#if plugin.keyword}<kbd class="chip">{plugin.keyword}</kbd>{/if}
                  </span>
                  <span class="hint">{plugin.description}</span>
                </div>
                <Toggle
                  checked={pluginEnabled(plugin.id)}
                  label="Enable {plugin.name}"
                  onchange={(on) => setPlugin(plugin.id, on)}
                />
              </div>
            {/each}

            <div class="row">
              <div class="label">
                <span class="name">Currency conversion</span>
                <span class="hint">
                  Calculator: “100 usd in eur”. Downloads the European Central Bank’s daily
                  rates in the background, at most once a day. Off keeps Sevak offline.
                </span>
              </div>
              <Toggle bind:checked={draft.calculator.currency} label="Convert currencies" />
            </div>
          </section>
          <p class="note">
            Web search engines are edited under “Web search”. Changes apply when you save.
          </p>
        {:else if active === "web"}
          <h1>Web search</h1>
          <p class="note">
            Type a keyword and your terms, such as <code>g rust traits</code>. <code>{"{query}"}</code>
            in the URL is replaced by what you type.
          </p>

          {#each draft.web_search as engine, i (i)}
            {@const errors = problems.engines[i] ?? {}}
            <section class="group engine" data-engine>
              <div class="engine-top">
                <div class="cell keyword">
                  <label class="mini" for="kw-{i}">Keyword</label>
                  <input
                    id="kw-{i}"
                    data-field="keyword"
                    class="input"
                    class:invalid={!!errors.keyword}
                    type="text"
                    value={engine.keyword}
                    oninput={(e) => renameKeyword(i, e.currentTarget.value)}
                    spellcheck="false"
                    autocomplete="off"
                    aria-invalid={!!errors.keyword}
                  />
                  {#if errors.keyword}<span class="msg error" role="alert">{errors.keyword}</span>{/if}
                </div>
                <div class="cell name-cell">
                  <label class="mini" for="nm-{i}">Name</label>
                  <input
                    id="nm-{i}"
                    class="input"
                    class:invalid={!!errors.name}
                    type="text"
                    bind:value={draft.web_search[i].name}
                    spellcheck="false"
                    autocomplete="off"
                    aria-invalid={!!errors.name}
                  />
                  {#if errors.name}<span class="msg error" role="alert">{errors.name}</span>{/if}
                </div>
                <button
                  type="button"
                  class="btn icon"
                  aria-label="Remove {engine.name || engine.keyword || 'engine'}"
                  title="Remove"
                  onclick={() => removeEngine(i)}>✕</button
                >
              </div>
              <div class="cell">
                <label class="mini" for="url-{i}">URL</label>
                <input
                  id="url-{i}"
                  class="input mono"
                  class:invalid={!!errors.url}
                  type="text"
                  bind:value={draft.web_search[i].url}
                  placeholder="https://example.com/search?q={'{query}'}"
                  spellcheck="false"
                  autocomplete="off"
                  aria-invalid={!!errors.url}
                />
                {#if errors.url}<span class="msg error" role="alert">{errors.url}</span>{/if}
              </div>
            </section>
          {/each}

          {#if draft.web_search.length === 0}
            <p class="empty">No engines. Web search is off until you add one.</p>
          {/if}
          <div class="inline">
            <button type="button" class="btn" onclick={addEngine}>Add engine</button>
          </div>
        {:else if active === "files"}
          <h1>Files</h1>

          <section class="group">
            <div class="field">
              <span class="name" id="dirs-label">Folders to search</span>
              <p class="hint">Files and folders below these are searchable.</p>
              {#if draft.files.directories.length === 0}
                <p class="empty">No folders yet.</p>
              {:else}
                <ul class="dirs" aria-labelledby="dirs-label">
                  {#each draft.files.directories as dir, i (dir)}
                    <li>
                      <span class="path" title={dir}>{dir}</span>
                      <button
                        type="button"
                        class="btn icon"
                        aria-label="Remove {dir}"
                        title="Remove"
                        onclick={() => removeDirectory(i)}>✕</button
                      >
                    </li>
                  {/each}
                </ul>
              {/if}
              <div class="inline">
                <button type="button" class="btn" onclick={addDirectory}>Add folder…</button>
              </div>
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="depth">Folder depth</label>
                <span class="hint">How many levels below each folder are indexed.</span>
                {#if problems.filesDepth}
                  <span class="msg error" role="alert">{problems.filesDepth}</span>
                {/if}
              </div>
              <input
                id="depth"
                class="input number"
                class:invalid={!!problems.filesDepth}
                type="number"
                min="0"
                max={MAX_DEPTH}
                step="1"
                bind:value={draft.files.max_depth}
                aria-invalid={!!problems.filesDepth}
              />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">Include hidden files</span>
                <span class="hint">Dot-files and dot-folders.</span>
              </div>
              <Toggle bind:checked={draft.files.include_hidden} label="Include hidden files" />
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="files-keyword">Keyword</label>
                <span class="hint">Type “keyword name” to search only files.</span>
                {#if problems.filesKeyword}
                  <span class="msg error" role="alert">{problems.filesKeyword}</span>
                {/if}
              </div>
              <input
                id="files-keyword"
                class="input number"
                class:invalid={!!problems.filesKeyword}
                type="text"
                bind:value={draft.files.keyword}
                spellcheck="false"
                autocomplete="off"
                aria-invalid={!!problems.filesKeyword}
              />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">Show in global results</span>
                <span class="hint">Also list matching files for plain searches, ranked lower.</span>
              </div>
              <Toggle bind:checked={draft.files.global} label="Show files in global results" />
            </div>
          </section>
        {:else if active === "linux"}
          <h1>Linux</h1>

          <section class="group">
            <div class="row">
              <div class="label">
                <span class="name">Use XWayland on Wayland</span>
                <span class="hint">
                  Native Wayland windows cannot position themselves, so Sevak draws through XWayland
                  to stay centered and typeable.
                </span>
                <span
                  class="msg"
                  class:warn={draft.linux.wayland_use_xwayland !== xwaylandAtOpen}
                  role="status"
                >
                  Restart Sevak to apply.
                </span>
              </div>
              <Toggle
                bind:checked={draft.linux.wayland_use_xwayland}
                label="Use XWayland on Wayland"
              />
            </div>
          </section>
        {/if}
      </div>
    </div>

    <footer class="footer">
      <div class="links">
        <button type="button" class="link" onclick={() => runFooter(openConfigFile)}>
          Open config file
        </button>
        <button type="button" class="link" onclick={() => runFooter(openLogDir)}>
          Reveal logs folder
        </button>
      </div>
      <div class="spacer"></div>
      {#if footerError}
        <span class="foot-msg error" role="alert">{footerError}</span>
      {:else if saveError}
        <span class="foot-msg error" role="alert" title={saveError}>{saveError}</span>
      {:else if problemCount > 0}
        <span class="foot-msg error" role="status">
          Fix {problemCount} problem{problemCount === 1 ? "" : "s"} to save
        </span>
      {/if}
      <button type="button" class="btn" onclick={cancel}>{dirty ? "Cancel" : "Close"}</button>
      <button type="button" class="btn primary" disabled={!canSave} onclick={save}>
        {saving ? "Saving…" : "Save"}
      </button>
    </footer>
  {/if}

  <div class="toast" class:show={toastVisible} role="status" aria-live="polite">
    {#if toastVisible}Saved{/if}
  </div>
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    font-size: 13.5px;
    line-height: 1.45;
    background: var(--bg);
    color: var(--fg);
  }

  .state {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 24px;
    color: var(--muted);
    text-align: center;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 184px 1fr;
  }

  /* Sidebar ---------------------------------------------------------------- */

  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 10px;
    border-right: 1px solid var(--border);
    background: var(--surface);
    overflow-y: auto;
  }

  .tab {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 7px 12px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .tab:hover {
    background: var(--tile);
  }

  .tab.active {
    background: var(--selected);
    font-weight: 600;
  }

  .tab:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--error);
  }

  /* Content ---------------------------------------------------------------- */

  .content {
    min-width: 0;
    padding: 20px 26px 28px;
    overflow-y: auto;
  }

  h1 {
    margin: 0 0 14px;
    font-size: 19px;
    font-weight: 650;
  }

  .group {
    border: 1px solid var(--border);
    border-radius: 11px;
    background: var(--surface);
  }

  .group + .group,
  .group + .inline,
  .group + .note,
  .note + .group {
    margin-top: 12px;
  }

  .row,
  .field {
    padding: 12px 16px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
  }

  .row + .row,
  .field + .row {
    border-top: 1px solid var(--border);
  }

  .label {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .name {
    font-weight: 560;
  }

  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--muted);
  }

  .field .hint {
    margin: 1px 0 8px;
  }

  .note {
    margin: 0 0 12px;
    font-size: 12.5px;
    color: var(--muted);
  }

  .empty {
    margin: 4px 0 10px;
    color: var(--muted);
    font-size: 12.5px;
  }

  code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
  }

  .chip {
    margin-left: 6px;
    padding: 0 6px;
    border: 1px solid var(--kbd-border);
    border-radius: 5px;
    background: var(--kbd-bg);
    color: var(--muted);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 11px;
    font-weight: 400;
  }

  .msg {
    font-size: 12px;
    line-height: 1.4;
  }

  .msg.error {
    color: var(--error);
  }

  .msg.warn {
    color: var(--warn);
  }

  .inline {
    display: flex;
    gap: 8px;
    margin-top: 10px;
  }

  .result {
    margin: 10px 0 0;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--tile);
    color: var(--fg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 11.5px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
    -webkit-user-select: text;
  }

  .result.fail {
    border-color: var(--error);
  }

  /* Controls --------------------------------------------------------------- */

  .input {
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
  }

  .input.mono {
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12.5px;
  }

  .input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .input.invalid {
    border-color: var(--error);
  }

  .input.number {
    width: 110px;
  }

  .select {
    min-width: 190px;
    max-width: 260px;
  }

  .btn {
    height: 32px;
    padding: 0 16px;
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
    opacity: 0.5;
    cursor: default;
  }

  .btn.primary {
    border-color: var(--accent-strong);
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
  }

  .btn.primary:hover:not(:disabled) {
    background: var(--accent-strong);
  }

  .btn.icon {
    flex: none;
    width: 32px;
    padding: 0;
    color: var(--muted);
  }

  .btn.icon:hover:not(:disabled) {
    color: var(--error);
    border-color: var(--error);
  }

  .segmented {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--input-border);
    border-radius: 9px;
    background: var(--input-bg);
  }

  .segmented label {
    position: relative;
    padding: 4px 14px;
    border-radius: 7px;
    cursor: pointer;
  }

  .segmented label.checked {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
  }

  .segmented input {
    position: absolute;
    inset: 0;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .segmented label:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .slider {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .slider input[type="range"] {
    width: 190px;
    height: 20px;
    margin: 0;
    background: transparent;
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
  }

  .slider input[type="range"]::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: var(--switch-off);
  }

  .slider input[type="range"]::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border: 0;
    border-radius: 50%;
    background: var(--accent-strong);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  }

  .slider input[type="range"]:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 4px;
    border-radius: 4px;
  }

  .slider output {
    min-width: 56px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  /* Web search engines -------------------------------------------------------- */

  .engine {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 14px 14px;
  }

  .engine-top {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  .cell {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }

  .cell .input {
    width: 100%;
  }

  .cell.keyword {
    flex: none;
    width: 92px;
  }

  .cell.name-cell {
    flex: 1;
  }

  .engine-top .btn.icon {
    margin-top: 20px;
  }

  .mini {
    font-size: 11.5px;
    color: var(--muted);
  }

  /* Files -------------------------------------------------------------------- */

  .dirs {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
  }

  .dirs li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 4px 4px 12px;
  }

  .dirs li + li {
    border-top: 1px solid var(--border);
  }

  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12.5px;
  }

  /* Footer ------------------------------------------------------------------- */

  .footer {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    border-top: 1px solid var(--border);
    background: var(--surface);
  }

  .links {
    display: flex;
    gap: 14px;
  }

  .link {
    padding: 4px 0;
    border: 0;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 12.5px;
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }

  .link:hover {
    color: var(--fg);
  }

  .link:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 3px;
  }

  .spacer {
    flex: 1;
  }

  .foot-msg {
    min-width: 0;
    max-width: 280px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 12.5px;
  }

  .foot-msg.error {
    color: var(--error);
  }

  /* Toast -------------------------------------------------------------------- */

  .toast {
    position: fixed;
    left: 50%;
    bottom: 64px;
    transform: translate(-50%, 8px);
    padding: 7px 18px;
    border-radius: 999px;
    background: var(--fg);
    color: var(--bg);
    font-size: 13px;
    font-weight: 560;
    box-shadow: var(--shadow);
    opacity: 0;
    pointer-events: none;
    transition:
      opacity 160ms ease,
      transform 160ms ease;
  }

  .toast.show {
    opacity: 1;
    transform: translate(-50%, 0);
  }

  @media (prefers-reduced-motion: reduce) {
    .toast {
      transition: none;
    }
  }

  @media (max-width: 620px) {
    .body {
      grid-template-columns: 148px 1fr;
    }

    .content {
      padding: 16px 16px 24px;
    }

    .row {
      flex-wrap: wrap;
    }
  }
</style>
