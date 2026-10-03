<script lang="ts">
  import { onMount } from "svelte";
  import type { Appearance } from "./settings-ipc";
  import {
    exportTheme,
    importTheme,
    listThemes,
    openThemesDir,
    saveTheme,
    useBuiltinTheme,
    type Result,
  } from "./theme-ipc";
  import ThemeColorRow from "./ThemeColorRow.svelte";
  import ThemeGallery from "./ThemeGallery.svelte";
  import ThemePreview from "./ThemePreview.svelte";
  import {
    clone,
    COLOR_FIELDS,
    contrastChecks,
    cssVars,
    deriveSecondary,
    emptySpec,
    FONT_SUGGESTIONS,
    fontFamilyError,
    History,
    LIMITS,
    miniVars,
    SIZE_DEFAULTS,
    type Mode,
    type Palette,
    type StoredTheme,
    type ThemeSpec,
    type ThemesDto,
  } from "./themes";

  let { appearance = $bindable() }: { appearance: Appearance } = $props();

  type Source = { kind: "builtin"; name: string } | { kind: "file"; file: string } | { kind: "new" };

  const systemMode = (): Mode =>
    typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";

  let themes = $state<ThemesDto | null>(null);
  let loadFailed = $state(false);
  /** The theme being edited. */
  let spec = $state<ThemeSpec>(emptySpec());
  /** `spec` as it was loaded or last saved, to tell whether it has unsaved edits. */
  let baseline = $state(JSON.stringify(emptySpec()));
  let source = $state<Source>({ kind: "new" });
  /** The variant the preview and the colors show. */
  let variant = $state<Mode>(systemMode());
  let message = $state<{ kind: "ok" | "warn" | "error"; text: string } | null>(null);
  let busy = $state(false);
  let overwritePending = $state(false);
  let showMore = $state(false);
  let installedFonts = $state<string[]>([]);
  let fontNote = $state("");

  const history = new History();
  /** Bumped whenever the history changes, so the buttons re-evaluate. */
  let historyVersion = $state(0);

  const builtin = $derived(themes?.builtin ?? []);
  const installed = $derived(themes?.installed ?? []);
  const defaults = $derived<Record<Mode, Palette>>({
    light: builtin.find((t) => t.spec.name === "Sevak Light")?.spec.light ?? {},
    dark: builtin.find((t) => t.spec.name === "Sevak Dark")?.spec.dark ?? {},
  });
  const other = $derived<Mode>(variant === "light" ? "dark" : "light");
  /** The variant whose colors are shown: `variant`, else the only one the theme has. */
  const editMode = $derived<Mode>(spec[variant] ? variant : spec[other] ? other : variant);
  const palette = $derived<Palette | null>(spec[editMode]);
  const checks = $derived(palette ? contrastChecks(palette, defaults[editMode]) : []);
  const failing = $derived(checks.filter((check) => !check.aa));
  const dirty = $derived(JSON.stringify(spec) !== baseline);
  const canUndo = $derived.by(() => {
    void historyVersion;
    return history.canUndo;
  });
  const canRedo = $derived.by(() => {
    void historyVersion;
    return history.canRedo;
  });
  const fontOptions = $derived(installedFonts.length > 0 ? installedFonts : FONT_SUGGESTIONS);
  const modes: Mode[] = ["light", "dark"];
  const fontError = $derived(fontFamilyError(spec.font.family));
  const builtinName = $derived(builtin.some((t) => t.spec.name.toLowerCase() === spec.name.trim().toLowerCase()));
  // A built-in that was only looked at is not a problem; once it is changed it needs its own name.
  const nameError = $derived(
    builtinName && (dirty || source.kind !== "builtin")
      ? "That is the name of a built-in theme; choose another one."
      : null,
  );
  const customThemes = $derived(installed.filter((t) => !builtin.some((b) => b.spec.name.toLowerCase() === t.spec.name.toLowerCase())));
  const mainFields = COLOR_FIELDS.filter((f) => !f.advanced);
  const moreFields = COLOR_FIELDS.filter((f) => f.advanced);
  const canSave = $derived(!busy && spec.name.trim() !== "" && !nameError && !fontError && palette !== null);
  const inUse = $derived(appearance.theme_file.trim());

  interface SizeControl {
    id: string;
    label: string;
    hint: string;
    unit: string;
    limits: readonly [number, number];
    fallback: number;
    get: (s: ThemeSpec) => number | null;
    set: (s: ThemeSpec, value: number | null) => void;
  }

  const sizes: SizeControl[] = [
    { id: "font_size", label: "Font size", hint: "Result titles; the bar scales with it", unit: "px", limits: LIMITS.font_size, fallback: SIZE_DEFAULTS.font_size, get: (s) => s.font.size, set: (s, v) => (s.font.size = v) },
    { id: "radius", label: "Corner radius", hint: "Roundness of the search bar", unit: "px", limits: LIMITS.radius, fallback: SIZE_DEFAULTS.radius, get: (s) => s.layout.radius, set: (s, v) => (s.layout.radius = v) },
    { id: "opacity", label: "Background opacity", hint: "Lower lets the desktop show through", unit: "%", limits: LIMITS.opacity, fallback: SIZE_DEFAULTS.opacity, get: (s) => s.layout.opacity, set: (s, v) => (s.layout.opacity = v) },
    { id: "row_height", label: "Row height", hint: "Height of a result row", unit: "px", limits: LIMITS.row_height, fallback: SIZE_DEFAULTS.row_height, get: (s) => s.layout.row_height, set: (s, v) => (s.layout.row_height = v) },
    { id: "search_size", label: "Search field size", hint: "Size of the text you type", unit: "px", limits: LIMITS.search_size, fallback: SIZE_DEFAULTS.search_size, get: (s) => s.layout.search_size, set: (s, v) => (s.layout.search_size = v) },
    { id: "icon_size", label: "Icon size", hint: "Size of the result icons", unit: "px", limits: LIMITS.icon_size, fallback: SIZE_DEFAULTS.icon_size, get: (s) => s.layout.icon_size, set: (s, v) => (s.layout.icon_size = v) },
    { id: "window_width", label: "Window width", hint: "Used while Settings keeps the default width; the preview is capped to the space here", unit: "px", limits: LIMITS.window_width, fallback: SIZE_DEFAULTS.window_width, get: (s) => s.layout.window_width, set: (s, v) => (s.layout.window_width = v) },
  ];

  const sameFile = (a: string, b: string) => {
    const normal = (path: string) => path.trim().replace(/\\/g, "/").replace(/^\.\//, "").toLowerCase();
    return normal(a) === normal(b);
  };

  // ---- loading ------------------------------------------------------------

  function load(theme: StoredTheme, how: Source) {
    spec = { ...emptySpec(), ...clone(theme.spec) };
    baseline = JSON.stringify(spec);
    source = how;
    history.clear();
    historyVersion++;
    overwritePending = false;
    message = theme.warnings.length
      ? { kind: "warn", text: `Some values in this theme were ignored: ${theme.warnings.join(" ")}` }
      : null;
    // Show the variant the theme has, else the one the system uses.
    if (!spec[variant] && spec[other]) variant = other;
  }

  async function refreshList() {
    const dto = await listThemes();
    if (dto) themes = dto;
    return dto;
  }

  onMount(async () => {
    const dto = await refreshList();
    if (!dto) {
      loadFailed = true;
      return;
    }
    const current = appearance.theme_file.trim();
    const mine = current ? dto.installed.find((t) => sameFile(t.file, current)) : undefined;
    if (mine) {
      load(mine, { kind: "file", file: mine.file });
      return;
    }
    const start = dto.builtin.find((t) => t.spec.name === (systemMode() === "dark" ? "Sevak Dark" : "Sevak Light"));
    if (start) load(start, { kind: "builtin", name: start.spec.name });
    if (current) {
      message = {
        kind: "warn",
        text: `The theme file "${current}" was not found in the themes folder, so no theme is applied.`,
      };
    }
  });

  function pick(theme: StoredTheme, isBuiltin: boolean) {
    const mine = isBuiltin ? installed.find((t) => t.spec.name.toLowerCase() === theme.spec.name.toLowerCase()) : undefined;
    // A built-in that was already written to the themes folder is that file.
    if (isBuiltin && !mine) load(theme, { kind: "builtin", name: theme.spec.name });
    else load(mine ?? theme, { kind: "file", file: (mine ?? theme).file });
  }

  // ---- editing ------------------------------------------------------------

  /** Records the current state for undo (merging edits of one field), then applies `change`. */
  function edit(key: string, change: (s: ThemeSpec) => void) {
    history.record(JSON.stringify(spec), key);
    historyVersion++;
    change(spec);
    overwritePending = false;
    message = null;
    // Editing a built-in makes a copy: the built-ins themselves stay as shipped.
    if (key !== "name" && builtinName) spec.name = `${spec.name.trim()} (copy)`;
  }

  function undo() {
    const previous = history.undo(JSON.stringify(spec));
    historyVersion++;
    if (previous !== null) spec = JSON.parse(previous) as ThemeSpec;
  }

  function redo() {
    const next = history.redo(JSON.stringify(spec));
    historyVersion++;
    if (next !== null) spec = JSON.parse(next) as ThemeSpec;
  }

  function reset() {
    edit("reset", (s) => {
      Object.assign(s, JSON.parse(baseline) as ThemeSpec);
    });
    message = null;
  }

  function setColor(value: string | undefined, key: string) {
    edit(`color:${editMode}:${key}`, (s) => {
      const target = (s[editMode] ??= {});
      if (value === undefined) delete target[key];
      else target[key] = value;
    });
  }

  function setSize(control: SizeControl, value: number | null) {
    edit(`size:${control.id}`, (s) => control.set(s, value));
  }

  function addVariant(mode: Mode) {
    edit(`variant:${mode}`, (s) => {
      s[mode] = clone(s[mode === "light" ? "dark" : "light"] ?? defaults[mode]);
    });
    variant = mode;
  }

  function removeVariant(mode: Mode) {
    edit(`variant:${mode}`, (s) => {
      s[mode] = null;
    });
    variant = mode === "light" ? "dark" : "light";
  }

  function derive() {
    edit(`derive:${editMode}`, (s) => {
      s[editMode] = deriveSecondary({ ...defaults[editMode], ...(s[editMode] ?? {}) }, editMode);
    });
  }

  async function loadInstalledFonts() {
    const query = (window as unknown as { queryLocalFonts?: () => Promise<{ family: string }[]> }).queryLocalFonts;
    if (!query) {
      fontNote = "This system's font list is not available here; type a font name instead.";
      return;
    }
    try {
      const fonts = await query.call(window);
      installedFonts = [...new Set(fonts.map((f) => f.family))].sort((a, b) => a.localeCompare(b));
      fontNote = `${installedFonts.length} installed fonts are suggested.`;
    } catch {
      fontNote = "Access to the font list was declined; type a font name instead.";
    }
  }

  // ---- saving and applying ------------------------------------------------

  /** Writes the theme as `themes/<name>.toml`. `null` (with a message) when it did not happen. */
  async function persist(): Promise<StoredTheme | null> {
    const name = spec.name.trim();
    if (name === "") {
      message = { kind: "error", text: "Give the theme a name first." };
      return null;
    }
    if (nameError || fontError) {
      message = { kind: "error", text: nameError ?? fontError ?? "" };
      return null;
    }
    const clash = installed.find(
      (t) => t.spec.name.toLowerCase() === name.toLowerCase() && !(source.kind === "file" && sameFile(source.file, t.file)),
    );
    if (clash && !overwritePending) {
      overwritePending = true;
      message = { kind: "warn", text: `A theme called "${clash.spec.name}" already exists. Click again to replace it.` };
      return null;
    }
    overwritePending = false;
    const result = await saveTheme($state.snapshot(spec) as ThemeSpec);
    if (!result.ok) {
      message = { kind: "error", text: result.error };
      return null;
    }
    const stored = result.value;
    await refreshList();
    spec = { ...emptySpec(), ...clone(stored.spec) };
    baseline = JSON.stringify(spec);
    source = { kind: "file", file: stored.file };
    return stored;
  }

  async function saveAs() {
    busy = true;
    const stored = await persist();
    busy = false;
    if (stored) {
      message = {
        kind: stored.warnings.length ? "warn" : "ok",
        text: stored.warnings.length
          ? `Saved ${stored.file}, but some values were ignored: ${stored.warnings.join(" ")}`
          : `Saved ${stored.file}.`,
      };
    }
  }

  async function apply() {
    busy = true;
    let stored: StoredTheme | null;
    if (source.kind === "builtin" && !dirty) {
      const result = await useBuiltinTheme(source.name);
      stored = result.ok ? result.value : null;
      if (!result.ok) message = { kind: "error", text: result.error };
      else await refreshList();
    } else if (source.kind === "file" && !dirty) {
      stored = { file: source.file, spec: clone(spec), warnings: [] };
    } else {
      stored = await persist();
    }
    busy = false;
    if (!stored) return;
    appearance.theme_file = stored.file;
    source = { kind: "file", file: stored.file };
    message = { kind: "ok", text: `${stored.spec.name || stored.file} will be used. Press Save below to keep it.` };
  }

  function removeTheme() {
    appearance.theme_file = "";
    message = { kind: "ok", text: "No theme file will be used. Press Save below to keep it." };
  }

  async function doImport() {
    busy = true;
    const result = await importTheme();
    busy = false;
    if (!result.ok) {
      message = { kind: "error", text: result.error };
      return;
    }
    if (!result.value) return;
    await refreshList();
    load(result.value, { kind: "file", file: result.value.file });
    message = { kind: "ok", text: `Imported ${result.value.spec.name}.` };
  }

  async function doExport() {
    busy = true;
    let outcome: Result<boolean>;
    if (source.kind === "file" && !dirty) {
      outcome = await exportTheme(source.file);
    } else if (source.kind === "builtin" && !dirty) {
      const written = await useBuiltinTheme(source.name);
      outcome = written.ok ? await exportTheme(written.value.file) : written;
    } else {
      outcome = { ok: false, error: "Save the theme first (Save as…), then export it." };
    }
    busy = false;
    if (!outcome.ok) message = { kind: "error", text: outcome.error };
    else if (outcome.value) message = { kind: "ok", text: "Exported the theme." };
  }

  async function openFolder() {
    const error = await openThemesDir();
    if (error) message = { kind: "error", text: error };
  }

  async function gallerySaved(stored: StoredTheme) {
    await refreshList();
    load(stored, { kind: "file", file: stored.file });
  }

  /** What a color the palette leaves out looks like: the Sevak default, or (selected text) the text color. */
  function fallbackFor(key: string): string {
    if (key === "selection_text") return palette?.text ?? defaults[editMode].text ?? "";
    return defaults[editMode][key] ?? "";
  }

  const ratio = (value: number) => `${value.toFixed(1)}:1`;
</script>

<section class="editor" aria-labelledby="theme-editor-title">
  <div class="top">
    <div class="label">
      <h2 id="theme-editor-title">Theme editor</h2>
      <p class="hint">
        Pick a theme to preview it, change what you like, then apply it. Themes are small files in
        your themes folder.
      </p>
    </div>
    <div class="in-use">
      {#if inUse}
        <span class="chip" title={inUse}>In use: <strong>{inUse}</strong></span>
        <button type="button" class="btn small" onclick={removeTheme}>Use none</button>
      {:else}
        <span class="chip">No theme file in use</span>
      {/if}
    </div>
  </div>

  {#if loadFailed}
    <p class="msg error" role="alert">The themes could not be listed.</p>
  {:else if !themes}
    <p class="hint pad">Loading themes…</p>
  {:else}
    <div class="block">
      <h3>Built-in themes</h3>
      <div class="grid">
        {#each builtin as theme (theme.file)}
          {@const isEdited = source.kind === "builtin" && source.name === theme.spec.name}
          <button
            type="button"
            class="theme"
            class:selected={isEdited || (source.kind === "file" && sameFile(source.file, theme.file))}
            aria-pressed={isEdited || (source.kind === "file" && sameFile(source.file, theme.file))}
            onclick={() => pick(theme, true)}
          >
            <span class="mini" use:cssVars={miniVars(theme.spec, variant, defaults)} aria-hidden="true">
              <span class="mini-bar"><i class="dot"></i><i class="line"></i></span>
              <span class="mini-row selected"><i class="tile"></i><i class="l1"></i></span>
              <span class="mini-row"><i class="tile"></i><i class="l1 short"></i></span>
            </span>
            <span class="theme-name">{theme.spec.name}</span>
            {#if sameFile(inUse, theme.file)}<span class="badge">In use</span>{/if}
          </button>
        {/each}
      </div>

      {#if customThemes.length > 0}
        <h3>Your themes</h3>
        <div class="grid">
          {#each customThemes as theme (theme.file)}
            {@const isEdited = source.kind === "file" && sameFile(source.file, theme.file)}
            <button
              type="button"
              class="theme"
              class:selected={isEdited}
              aria-pressed={isEdited}
              onclick={() => pick(theme, false)}
            >
              <span class="mini" use:cssVars={miniVars(theme.spec, variant, defaults)} aria-hidden="true">
                <span class="mini-bar"><i class="dot"></i><i class="line"></i></span>
                <span class="mini-row selected"><i class="tile"></i><i class="l1"></i></span>
                <span class="mini-row"><i class="tile"></i><i class="l1 short"></i></span>
              </span>
              <span class="theme-name">{theme.spec.name}</span>
              {#if sameFile(inUse, theme.file)}<span class="badge">In use</span>{/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="block">
      <div class="preview-head">
        <h3>Preview</h3>
        <div class="segmented" role="radiogroup" aria-label="Variant to preview">
          {#each modes as mode (mode)}
            <label class:checked={variant === mode}>
              <input type="radio" name="variant" value={mode} bind:group={variant} />
              <span>{mode === "light" ? "Light" : "Dark"}{spec[mode] ? "" : " (none)"}</span>
            </label>
          {/each}
        </div>
      </div>
      {#if palette}
        <ThemePreview {spec} {palette} defaults={defaults[editMode]} mode={editMode} />
      {:else}
        <p class="hint pad">This theme has no colors yet. Add a variant to start editing.</p>
      {/if}
      <div class="variant-actions">
        {#if !spec[variant]}
          <span class="hint">
            {spec[other]
              ? `This theme has no ${variant} variant, so its ${other} colors are used in ${variant} mode too.`
              : "No colors yet."}
          </span>
          <button type="button" class="btn small" onclick={() => addVariant(variant)}>Add {variant} variant</button>
        {:else if spec[other]}
          <button type="button" class="btn small" onclick={() => removeVariant(variant)}>Remove {variant} variant</button>
        {:else}
          <span class="hint">One palette is used in light and dark mode.</span>
          <button type="button" class="btn small" onclick={() => addVariant(other)}>Add {other} variant</button>
        {/if}
      </div>

      {#if palette}
        <div class="checks" aria-label="Contrast">
          <ul>
            {#each checks as check (check.id)}
              <li class:fail={!check.aa}>
                <span class="mark" aria-hidden="true">{check.aa ? "✓" : "!"}</span>
                <span>{check.label}</span>
                <span class="ratio">{ratio(check.ratio)}</span>
                <span class="verdict">{check.aa ? "AA" : "Below AA"}</span>
              </li>
            {/each}
          </ul>
          {#if failing.length > 0}
            <p class="msg warn" role="status">
              {failing.map((c) => c.label.toLowerCase()).join(", ")}:
              contrast is below 4.5:1 (WCAG AA), so some people will find this hard to read.
            </p>
          {/if}
        </div>
      {/if}
    </div>

    <div class="block">
      <div class="row">
        <div class="label">
          <label class="name" for="theme-name">Name</label>
          {#if nameError}<span class="msg error" role="alert">{nameError}</span>{/if}
        </div>
        <input
          id="theme-name"
          class="input"
          class:invalid={!!nameError}
          type="text"
          maxlength="60"
          value={spec.name}
          placeholder="My theme"
          aria-invalid={!!nameError}
          oninput={(e) => {
            const value = e.currentTarget.value;
            edit("name", (s) => (s.name = value));
          }}
        />
      </div>
      <div class="row">
        <div class="label"><label class="name" for="theme-author">Author</label></div>
        <input
          id="theme-author"
          class="input"
          type="text"
          maxlength="60"
          value={spec.author}
          oninput={(e) => {
            const value = e.currentTarget.value;
            edit("author", (s) => (s.author = value));
          }}
        />
      </div>

      <div class="actions">
        <div class="history">
          <button type="button" class="btn" disabled={!canUndo} onclick={undo}>Undo</button>
          <button type="button" class="btn" disabled={!canRedo} onclick={redo}>Redo</button>
          <button type="button" class="btn" disabled={!dirty} onclick={reset}>Reset</button>
        </div>
        <div class="primary">
          <button type="button" class="btn" disabled={!canSave} onclick={saveAs}>
            {overwritePending ? "Replace" : "Save as…"}
          </button>
          <button type="button" class="btn accent" disabled={!canSave} onclick={apply}>
            {overwritePending ? "Replace and apply" : "Apply"}
          </button>
        </div>
      </div>
      {#if message}
        <p class="msg" class:error={message.kind === "error"} class:warn={message.kind === "warn"} class:ok={message.kind === "ok"} role={message.kind === "error" ? "alert" : "status"}>
          {message.text}
        </p>
      {/if}
      <div class="files">
        <button type="button" class="btn small" disabled={busy} onclick={doImport}>Import…</button>
        <button type="button" class="btn small" disabled={busy} onclick={doExport}>Export…</button>
        <button type="button" class="btn small" onclick={openFolder}>Open themes folder</button>
      </div>
    </div>

    {#if palette}
      <div class="block">
        <h3>Colors ({editMode})</h3>
        <div class="colors">
          {#each mainFields as field (field.key)}
            <ThemeColorRow {field} value={palette[field.key]} fallback={fallbackFor(field.key)} onchange={setColor} />
          {/each}
        </div>
        <button type="button" class="more" aria-expanded={showMore} onclick={() => (showMore = !showMore)}>
          {showMore ? "Fewer colors" : "More colors"}
        </button>
        {#if showMore}
          <div class="colors">
            {#each moreFields as field (field.key)}
              <ThemeColorRow {field} value={palette[field.key]} fallback={fallbackFor(field.key)} onchange={setColor} />
            {/each}
          </div>
          <div class="derive">
            <button type="button" class="btn small" onclick={derive}>Derive from text, background and accent</button>
            <span class="hint">Recomputes borders, tiles, key caps, panels, fields, the accent's hover shade and the selection.</span>
          </div>
        {/if}
      </div>
    {/if}

    <div class="block">
      <h3>Sizes and font</h3>
      <div class="colors">
        {#each sizes as control (control.id)}
          {@const value = control.get(spec)}
          <div class="row slider-row">
            <div class="label">
              <label class="name" for="size-{control.id}">{control.label}</label>
              <span class="hint">{control.hint}</span>
            </div>
            <div class="slider">
              <input
                id="size-{control.id}"
                type="range"
                min={control.limits[0]}
                max={control.limits[1]}
                step="1"
                value={value ?? control.fallback}
                oninput={(e) => setSize(control, Number(e.currentTarget.value))}
              />
              <output for="size-{control.id}">{value ?? control.fallback}{control.unit}</output>
              <button
                type="button"
                class="btn small"
                disabled={value === null}
                title="Use the default"
                aria-label="Use the default {control.label.toLowerCase()}"
                onclick={() => setSize(control, null)}>Default</button
              >
            </div>
          </div>
        {/each}
        <div class="row">
          <div class="label">
            <label class="name" for="theme-font">Font</label>
            <span class="hint">A comma-separated list, such as Fira Sans, sans-serif. Empty uses the system font.</span>
            {#if fontError}<span class="msg error" role="alert">{fontError}</span>{/if}
            {#if fontNote}<span class="hint">{fontNote}</span>{/if}
          </div>
          <div class="font-controls">
            <input
              id="theme-font"
              class="input"
              class:invalid={!!fontError}
              type="text"
              list="theme-fonts"
              value={spec.font.family}
              spellcheck="false"
              autocomplete="off"
              aria-invalid={!!fontError}
              oninput={(e) => {
                const value = e.currentTarget.value;
                edit("font", (s) => (s.font.family = value));
              }}
            />
            <datalist id="theme-fonts">
              {#each fontOptions as font (font)}
                <option value={font}></option>
              {/each}
            </datalist>
            <button type="button" class="btn small" onclick={loadInstalledFonts}>Installed fonts</button>
          </div>
        </div>
      </div>
    </div>

    <ThemeGallery oninstalled={gallerySaved} />
  {/if}
</section>

<style>
  .editor {
    margin-top: 12px;
    border: 1px solid var(--border);
    border-radius: 11px;
    background: var(--surface);
  }

  .top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 16px;
  }

  h2 {
    margin: 0 0 2px;
    font-size: 15px;
    font-weight: 620;
  }

  h3 {
    margin: 0 0 8px;
    font-size: 12.5px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--muted);
  }

  .block {
    padding: 12px 16px;
    border-top: 1px solid var(--border);
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

  .label {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .name {
    font-weight: 560;
  }

  .in-use {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 50%;
  }

  .chip {
    padding: 3px 9px;
    border-radius: 999px;
    background: var(--kbd-bg);
    border: 1px solid var(--kbd-border);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Theme cards. */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(118px, 1fr));
    gap: 10px;
    margin-bottom: 12px;
  }

  .theme {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px;
    border: 1px solid var(--input-border);
    border-radius: 9px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .theme:hover {
    border-color: var(--accent-strong);
  }

  .theme.selected {
    border-color: var(--accent-strong);
    box-shadow: 0 0 0 2px var(--accent);
  }

  .theme:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .theme-name {
    padding: 0 2px 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge {
    position: absolute;
    top: 10px;
    right: 10px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 10.5px;
    font-weight: 600;
  }

  /* A miniature of the launcher, drawn with the theme's colors only. */
  .mini {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--bg);
    box-shadow: var(--shadow);
    color: var(--fg);
  }

  .mini i {
    display: block;
    border-radius: 3px;
  }

  .mini-bar {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 14px;
  }

  .mini-bar .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--accent);
  }

  .mini-bar .line {
    width: 38%;
    height: 4px;
    background: var(--fg);
  }

  .mini-row {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 14px;
    padding: 0 3px;
    border-radius: 4px;
  }

  .mini-row.selected {
    background: var(--selected);
  }

  .mini-row .tile {
    width: 8px;
    height: 8px;
    background: var(--tile);
    border: 1px solid var(--border);
  }

  .mini-row .l1 {
    width: 52%;
    height: 4px;
    background: var(--selected-fg, var(--fg));
  }

  .mini-row:not(.selected) .l1 {
    background: var(--fg);
  }

  .mini-row .l1.short {
    width: 36%;
    background: var(--muted);
  }

  /* Preview. */
  .preview-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 8px;
  }

  .preview-head h3 {
    margin: 0;
  }

  .segmented {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--input-border);
    border-radius: 8px;
    background: var(--input-bg);
  }

  .segmented label {
    position: relative;
    padding: 4px 12px;
    border-radius: 6px;
    font-size: 12.5px;
    cursor: pointer;
  }

  .segmented label.checked {
    background: var(--accent);
    color: var(--on-accent);
  }

  .segmented input {
    position: absolute;
    opacity: 0;
    inset: 0;
    cursor: pointer;
  }

  .segmented label:has(input:focus-visible) {
    outline: 2px solid var(--accent-strong);
    outline-offset: 2px;
  }

  .variant-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 8px;
  }

  /* Contrast. */
  .checks {
    margin-top: 10px;
  }

  .checks ul {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px 16px;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 12.5px;
  }

  .checks li {
    display: grid;
    grid-template-columns: 16px 1fr auto auto;
    align-items: center;
    gap: 6px;
  }

  .mark {
    color: var(--ok);
    font-weight: 700;
  }

  .fail .mark,
  .fail .verdict {
    color: var(--error);
  }

  .ratio {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  .verdict {
    min-width: 62px;
    text-align: right;
    font-weight: 600;
    color: var(--ok);
  }

  /* Rows and controls. */
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 8px 0;
  }

  .colors {
    margin: 0 -16px;
  }

  .colors .row {
    padding: 8px 16px;
  }

  .colors .row + .row {
    border-top: 1px solid var(--border);
  }

  .colors :global(.color-row:first-child) {
    border-top: 0;
  }

  .msg {
    margin: 6px 0 0;
    font-size: 12px;
    line-height: 1.4;
  }

  .msg.error {
    color: var(--error);
  }

  .msg.warn {
    color: var(--warn);
  }

  .msg.ok {
    color: var(--ok);
  }

  .label .msg {
    margin: 0;
  }

  .input {
    width: 200px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
  }

  .input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .input.invalid {
    border-color: var(--error);
  }

  .font-controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .font-controls .input {
    width: 200px;
  }

  .btn {
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

  .btn.small {
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
  }

  .btn.accent {
    border-color: var(--accent-strong);
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 600;
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

  .actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 8px;
  }

  .history,
  .primary,
  .files {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .files {
    margin-top: 10px;
  }

  .more {
    margin: 8px 0 0;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-strong);
    font: inherit;
    font-size: 12.5px;
    text-decoration: underline;
    cursor: pointer;
  }

  .derive {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
  }

  .slider {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .slider input[type="range"] {
    width: 170px;
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
    min-width: 50px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  @media (max-width: 700px) {
    .checks ul {
      grid-template-columns: 1fr;
    }
  }
</style>
