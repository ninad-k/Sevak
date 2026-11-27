<script lang="ts">
  import { onMount, tick } from "svelte";
  import AppearanceExtras from "./lib/AppearanceExtras.svelte";
  import AiPage from "./lib/settings-pages/AiPage.svelte";
  import ClipboardPage from "./lib/settings-pages/ClipboardPage.svelte";
  import FilesExtras from "./lib/settings-pages/FilesExtras.svelte";
  import IntegrationsPage from "./lib/settings-pages/IntegrationsPage.svelte";
  import ListEditor from "./lib/settings-pages/ListEditor.svelte";
  import SystemPage from "./lib/settings-pages/SystemPage.svelte";
  import TasksMediaPage from "./lib/settings-pages/TasksMediaPage.svelte";
  import WindowsPage from "./lib/settings/WindowsPage.svelte";
  import { tidyPluginSettings } from "./lib/settings-pages/tidy";
  import HotkeyField from "./lib/HotkeyField.svelte";
  import HelpPage from "./lib/HelpPage.svelte";
  import BackupPage from "./lib/settings/BackupPage.svelte";
  import HotkeyList from "./lib/HotkeyList.svelte";
  import ThemeEditor from "./lib/ThemeEditor.svelte";
  import Toggle from "./lib/Toggle.svelte";
  import ExtensionsPage from "./lib/extensions/ExtensionsPage.svelte";
  import { onSettingsPage } from "./lib/extensions/extensions-ipc";
  import GalleryPage from "./lib/workflows/GalleryPage.svelte";
  import WorkflowsPage from "./lib/workflows/WorkflowsPage.svelte";
  import { getStatus, onStatus, type Status } from "./lib/ipc";
  import {
    closeSettings,
    getSettings,
    getStartupStatus,
    openConfigFile,
    openLogDir,
    pickDirectory,
    restoreTakeover,
    repairStartup,
    saveSettings,
    setupWaylandHotkey,
    takeOverHotkey,
    validateHotkey,
    fallbackList,
    type Config,
    type Outcome,
    type PluginInfo,
    type SettingsDto,
    type StartupStatus,
  } from "./lib/settings-ipc";
  import { displayAccelerator } from "./lib/accelerator";
  import { applyAppearance } from "./lib/appearance";
  import { applyTheme, resolveSettingsTheme } from "./lib/theme";
  import { LANGUAGES, currentLanguage, formatList, formatNumber, resolveLanguage, setLanguage, t, tHtml } from "./lib/i18n.svelte";
  import {
    MAX_DEPTH,
    MAX_FILE_RESULTS,
    MAX_WIDTH,
    MIN_WIDTH,
    totalProblems,
    validate,
    type SectionId,
  } from "./lib/validate";

  const emptyConfig = (): Config => ({
    general: {
      hotkey: "",
      actions_hotkey: "",
      accept_injected_hotkeys: false,
      hide_on_blur: true,
      launch_at_login: false,
      check_for_updates: true,
      update_channel: "stable",
      language: "auto",
    },
    window: { width: 720 },
    linux: { wayland_use_xwayland: true },
    actions: { use_primary_selection: true, use_clipboard_fallback: false },
    paste: { restore_clipboard: false },
    search: { max_results: 8, fallback_web_search: "", query_history: true },
    appearance: {
      theme: "system",
      accent: "",
      font_size: 15,
      font_family: "",
      opacity: 100,
      blur: false,
      radius: 14,
      theme_file: "",
      custom_css: "",
      settings_theme: "follow",
      open_animation: "fade",
      animation_ms: 150,
    },
    plugins: { disabled: [] },
    calculator: { currency: false },
    files: {
      directories: [],
      max_depth: 4,
      include_hidden: false,
      keyword: "",
      global: true,
      use_os_index: true,
      index_keyword: "ff",
      content_keyword: "in",
      allow_network_paths: false,
      sort: "relevance",
      max_results: 0,
    },
    snippets: {
      auto_expand: false,
      prefix: "",
      expand_on: "immediate",
      case_sensitive: true,
      ignore_apps: [],
      expand_in_terminals: false,
      expand_in_browsers: false,
    },
    bookmarks: { browsers: [], keyword: "", global: true },
    file_buffer: { keep_between_shows: false },
    clipboard: {
      enabled: false,
      max_items: 200,
      max_item_bytes: 64 * 1024,
      ignore_apps: [],
      default_ignore_apps: true,
      images: true,
      files: true,
      max_image_bytes: 10 * 1024 * 1024,
      encrypt: true,
      viewer_rows: 20,
      preview_chars: 100,
    },
    contacts: { enabled: false, keyword: "c", use_system: true, vcard_files: [], show_photos: true },
    onepassword: { enabled: false, keyword: "1p", op_path: "", account: "", cache_minutes: 10 },
    dictionary: { define_keyword: "define", spell_keyword: "spell", use_system: true },
    ai: {
      enabled: false,
      keyword: "ai",
      provider: "ollama",
      model: "",
      base_url: "",
      system_prompt: "",
      max_tokens: 512,
      timeout_secs: 60,
    },
    system: { confirm: true, disabled: [] },
    tasks: { confirm: true, disabled: [], keyword: "t", global: true },
    media: { keyword: "play", global: true, now_playing: true },
    window_management: { enabled: true, keyword: "win", switcher_keyword: "w", gap: 0, global: false },
    shell: { terminal: "", shell: "", keep_open: true },
    clock: { keyword: "time", cities: [] },
    passwords: { keyword: "pass", length: 20, words: 6, symbols: true },
    web_search: [],
    hotkey: [],
    ssh: { keyword: "ssh", known_hosts: false },
    projects: { keyword: "code", editors: [] },
    calendar: { keyword: "cal", files: [], days: 7 },
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
  /** Hotkey entries whose key Rust cannot parse (reported by the list). */
  let hotkeyParseProblems = $state(0);

  /** The workflow pages keep their own files and are not part of the config form. */
  type PageId = SectionId | "workflows" | "gallery" | "extensions" | "backup" | "help";
  let active = $state<PageId>("general");
  let hotkeyError = $state<string | null>(null);
  let actionsHotkeyError = $state<string | null>(null);
  let saving = $state(false);
  let startupBusy = $state<"check" | "repair" | null>(null);
  let startupError = $state<string | null>(null);
  let startupMessage = $state<string | null>(null);
  let saveError = $state<string | null>(null);
  let toastVisible = $state(false);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let footerError = $state<string | null>(null);
  let waylandBusy = $state(false);
  let waylandResult = $state<Outcome | null>(null);
  let takeoverBusy = $state(false);
  let takeoverResult = $state<Outcome | null>(null);
  let content: HTMLElement | undefined = $state();

  const ready = $derived(loaded !== null);
  const display = $derived(loaded?.display ?? "unknown");
  const isWayland = $derived(display === "wayland");
  const showLinux = $derived(display === "x11" || display === "wayland");
  const dirty = $derived(ready && JSON.stringify(draft) !== baseline);
  const problems = $derived(validate(draft));
  const problemCount = $derived(
    totalProblems(problems) +
      (hotkeyError ? 1 : 0) +
      (actionsHotkeyError ? 1 : 0) +
      hotkeyParseProblems,
  );
  const canSave = $derived(ready && dirty && problemCount === 0 && !saving && startupBusy === null);
  const startupDraftChanged = $derived(loaded !== null && draft.general.launch_at_login !== loaded.config.general.launch_at_login);
  const otherCopyStartup = $derived(!!loaded && !loaded.startup.error && loaded.startup.registered && !!loaded.startup.other_copy);
  const needsStartupRepair = $derived(!!loaded?.config.general.launch_at_login && (!!loaded.startup.error || !loaded.startup.enabled || otherCopyStartup));
  /** Startup is registered for this copy but switched off: "Enable again" rather than "Repair startup". */
  const startupEnableAgain = $derived(!!loaded?.startup.registered && !loaded.startup.error && !otherCopyStartup);
  const startupRepairLabel = $derived(startupEnableAgain ? t("settings.startup.enableAgain") : t("settings.startup.repair"));
  const startupStatusLabel = $derived.by(() => {
    const startup = loaded?.startup;
    if (!startup || startup.error) return t("settings.startup.state.unavailable");
    if (otherCopyStartup) return t("settings.startup.state.otherCopy");
    if (startup.enabled) return loaded?.config.general.launch_at_login ? t("settings.startup.state.enabled") : t("settings.startup.state.enabledOutside");
    if (startup.registered) return t("settings.startup.state.disabledOutside");
    return loaded?.config.general.launch_at_login ? t("settings.startup.state.missing") : t("settings.startup.state.off");
  });

  const sections = $derived(
    (
      [
        { id: "general", label: t("settings.sections.general") },
        { id: "hotkeys", label: t("settings.sections.hotkeys") },
        { id: "appearance", label: t("settings.sections.appearance") },
        { id: "search", label: t("settings.sections.search") },
        { id: "plugins", label: t("settings.sections.plugins") },
        { id: "workflows", label: t("settings.sections.workflows") },
        { id: "gallery", label: t("settings.sections.gallery") },
        { id: "extensions", label: t("settings.sections.extensions") },
        { id: "web", label: t("settings.sections.web") },
        { id: "files", label: t("settings.sections.files") },
        { id: "clipboard", label: t("settings.sections.clipboard") },
        { id: "tasks", label: t("settings.sections.tasks") },
        { id: "windows", label: t("settings.sections.windows") },
        { id: "integrations", label: t("settings.sections.integrations") },
        { id: "ai", label: t("settings.sections.ai") },
        { id: "system", label: t("settings.sections.system") },
        ...(showLinux ? [{ id: "linux", label: t("settings.sections.linux") }] : []),
        { id: "backup", label: t("settings.sections.backup") },
        { id: "help", label: t("settings.sections.help") },
      ] as { id: PageId; label: string }[]
    ).map((section) => ({
      ...section,
      problems:
        (section.id in problems.count ? problems.count[section.id as SectionId] : 0) +
        (section.id === "general" && hotkeyError ? 1 : 0) +
        (section.id === "general" && actionsHotkeyError ? 1 : 0) +
        (section.id === "hotkeys" ? hotkeyParseProblems : 0),
    })),
  );

  const themes = $derived([
    { value: "system", label: t("settings.appearance.system") },
    { value: "light", label: t("settings.appearance.light") },
    { value: "dark", label: t("settings.appearance.dark") },
  ] as const);

  const engineKeywords = $derived(
    [...new Set(draft.web_search.map((e) => e.keyword.trim()).filter((k) => k !== ""))].map(
      (keyword) => ({
        keyword,
        name: draft.web_search.find((e) => e.keyword.trim() === keyword)?.name.trim() ?? "",
      }),
    ),
  );
  // config.toml may list several fallbacks; the select shows one, and leaves a
  // list alone until the user picks an engine.
  const MANY_FALLBACKS = "__many__";
  const fallbackKeywords = $derived(fallbackList(draft.search.fallback_web_search));
  const fallbackMany = $derived(fallbackKeywords.length > 1);
  const fallbackMissing = $derived(
    fallbackKeywords.length === 1 && !engineKeywords.some((e) => e.keyword === fallbackKeywords[0]),
  );
  function getFallback(): string {
    return fallbackMany ? MANY_FALLBACKS : (fallbackKeywords[0] ?? "");
  }
  function setFallback(value: string) {
    if (value !== MANY_FALLBACKS) draft.search.fallback_web_search = value;
  }
  /** Rewrites the fallback keywords (string or list form); `null` drops one. */
  function mapFallback(change: (keyword: string) => string | null) {
    const current = draft.search.fallback_web_search;
    if (Array.isArray(current)) {
      draft.search.fallback_web_search = current.flatMap((k) => change(k.trim()) ?? []);
    } else {
      draft.search.fallback_web_search = change(current.trim()) ?? "";
    }
  }

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
          description: t("settings.plugins.webDescription", {
            name,
            example: `${keyword} <${t("settings.plugins.terms")}>`,
          }),
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
  /** A plugin switched off under Plugins: its page still edits the settings, and says so. */
  const pluginOff = (id: string) => !pluginEnabled(id);
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

  // The Universal Actions shortcut may be empty (off); otherwise the same parser applies.
  let actionsSeq = 0;
  $effect(() => {
    const value = draft.general.actions_hotkey.trim();
    if (!ready) return;
    const mine = ++actionsSeq;
    if (value === "") {
      actionsHotkeyError = null;
      return;
    }
    const timer = setTimeout(async () => {
      const error = await validateHotkey(value);
      if (mine === actionsSeq) actionsHotkeyError = error;
    }, 150);
    return () => clearTimeout(timer);
  });

  // Preview the theme while choosing it; saving makes it stick for every window.
  $effect(() => {
    if (ready) {
      applyTheme(resolveSettingsTheme(draft.appearance.settings_theme, draft.appearance.theme));
    }
  });

  // Switch the language as soon as it is picked, like the theme.
  $effect(() => {
    if (ready) setLanguage(draft.general.language);
  });
  /** The language `auto` stands for on this computer, named in itself. */
  const autoLanguageName = $derived(
    LANGUAGES.find((l) => l.code === resolveLanguage("auto", typeof navigator === "undefined" ? [] : navigator.languages ?? []))?.name ?? "English",
  );

  // The rest of the appearance (accent, fonts, custom stylesheet) follows the saved config.
  $effect(() => applyAppearance(status?.appearance));

  const registrationWarning = $derived(
    !isWayland &&
      status?.hotkey.error &&
      status.hotkey.accelerator === draft.general.hotkey.trim()
      ? status.hotkey.error
      : null,
  );

  /** How the saved shortcut reaches Sevak (keyboard hook, Spotlight's shortcut off...), while the field still shows it. */
  const hotkeyNote = $derived(
    status?.hotkey.note && status.hotkey.accelerator === draft.general.hotkey.trim()
      ? status.hotkey.note
      : null,
  );
  const canTakeOver = $derived(
    !!status?.hotkey.can_take_over && status.hotkey.accelerator === draft.general.hotkey.trim(),
  );
  const canRestore = $derived(!!status?.hotkey.can_restore);
  const platform = $derived(loaded?.platform ?? "linux");
  const takeoverLabel = $derived(
    t("settings.general.takeOver", { shortcut: displayAccelerator(draft.general.hotkey.trim(), platform) }),
  );
  const restoreLabel = $derived(
    platform === "macos"
      ? t("settings.general.restoreSpotlight")
      : t("settings.general.restoreGnome"),
  );

  const actionsRegistrationWarning = $derived(
    !isWayland &&
      status?.actions_hotkey?.error &&
      status.actions_hotkey.key === draft.general.actions_hotkey.trim()
      ? status.actions_hotkey.error
      : null,
  );

  async function load() {
    const dto = await getSettings();
    if (!dto) {
      loadFailed = true;
      return;
    }
    if (dto.startup.saved !== null && dto.startup.saved !== undefined) dto.config.general.launch_at_login = dto.startup.saved;
    loaded = dto;
    draft = clone(dto.config);
    baseline = JSON.stringify(dto.config);
    xwaylandAtOpen ??= dto.config.linux.wayland_use_xwayland;
  }

  /**
   * Show a fresh OS status without discarding the draft. The installer or
   * `--set-startup` may have changed the saved preference since Settings
   * loaded: follow it, and move the switch too unless the user already did.
   */
  function applyStartupStatus(next: StartupStatus) {
    if (!loaded) return;
    loaded.startup = next;
    const saved = next.saved;
    if (saved === null || saved === undefined || saved === loaded.config.general.launch_at_login) return;
    const untouched = draft.general.launch_at_login === loaded.config.general.launch_at_login;
    loaded.config.general.launch_at_login = saved;
    baseline = JSON.stringify(loaded.config);
    if (untouched) draft.general.launch_at_login = saved;
    startupMessage = saved ? t("settings.startup.changedOutsideOn") : t("settings.startup.changedOutsideOff");
  }

  function showToast() {
    toastVisible = true;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toastVisible = false), 2200);
  }

  /** The form is disabled while saving; give focus back to where it was (Ctrl+S from a field). */
  async function finishSaving(focused: HTMLElement | null) {
    saving = false;
    await tick();
    if (focused?.isConnected && (!document.activeElement || document.activeElement === document.body)) focused.focus();
  }

  async function save() {
    if (!canSave) return;
    const focused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    saving = true;
    startupError = null;
    startupMessage = null;
    saveError = null;
    const payload = clone($state.snapshot(draft)) as Config;
    payload.general.hotkey = payload.general.hotkey.trim();
    payload.general.actions_hotkey = payload.general.actions_hotkey.trim();
    payload.files.keyword = payload.files.keyword.trim();
    payload.files.index_keyword = payload.files.index_keyword.trim();
    payload.files.content_keyword = payload.files.content_keyword.trim();
    payload.files.directories = payload.files.directories.map((dir) => dir.trim());
    tidyPluginSettings(payload);
    payload.search.fallback_web_search = Array.isArray(payload.search.fallback_web_search)
      ? payload.search.fallback_web_search.map((k) => k.trim())
      : payload.search.fallback_web_search.trim();
    payload.appearance.accent = payload.appearance.accent.trim();
    payload.appearance.font_family = payload.appearance.font_family.trim();
    payload.appearance.theme_file = payload.appearance.theme_file.trim();
    payload.appearance.custom_css = payload.appearance.custom_css.trim();
    payload.hotkey = payload.hotkey.map((binding) =>
      binding.run != null
        ? { key: binding.key.trim(), run: binding.run.trim() }
        : { key: binding.key.trim(), query: binding.query ?? "" },
    );
    payload.web_search = payload.web_search.map((engine) => ({
      keyword: engine.keyword.trim(),
      name: engine.name.trim(),
      url: engine.url.trim(),
    }));
    const error = await saveSettings(payload);
    if (error) {
      saveError = error;
      await finishSaving(focused);
      return;
    }
    await load();
    await finishSaving(focused);
    showToast();
  }

  async function checkStartup() {
    if (!loaded || saving || startupBusy) return;
    startupBusy = "check";
    startupError = null;
    startupMessage = null;
    applyStartupStatus(await getStartupStatus());
    startupBusy = null;
  }

  async function enableStartupAgain() {
    if (!loaded || saving || startupBusy || startupDraftChanged || !needsStartupRepair) return;
    startupBusy = "repair";
    startupError = null;
    startupMessage = null;
    const result = await repairStartup();
    if (result.status) {
      // Keep every unsaved field and the baseline. Repair never saves config.
      loaded.startup = result.status;
      startupMessage = t("settings.startup.enabledMessage");
    } else {
      // A failed repair (or its rollback) may have changed the OS entry, and
      // the saved preference may have changed outside Settings: show both.
      applyStartupStatus(await getStartupStatus());
      startupError = result.error;
    }
    startupBusy = null;
  }

  async function cancel() {
    if (saving || startupBusy) return;
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

  async function runTakeover(action: () => Promise<Outcome>) {
    takeoverBusy = true;
    takeoverResult = null;
    takeoverResult = await action();
    takeoverBusy = false;
    status = (await getStatus()) ?? status;
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
    if (!stillDefined) mapFallback((k) => (k === removed.keyword.trim() ? null : k));
  }

  function renameKeyword(index: number, value: string) {
    const old = draft.web_search[index].keyword;
    draft.web_search[index].keyword = value;
    // The fallback follows a renamed engine instead of silently dangling.
    if (old.trim() !== "") mapFallback((k) => (k === old.trim() ? value.trim() : k));
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
    // The launcher's `ext` rows open this window on the Extensions page.
    const stopPage = onSettingsPage((page) => {
      if (page === "extensions") active = "extensions";
    });
    return () => {
      stopPage();
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
  <title>{t("settings.title")}</title>
</svelte:head>

<div class="app">
  {#if loadFailed}
    <div class="state" role="alert">
      {t("settings.loadFailed")}
    </div>
  {:else if !ready}
    <div class="state" role="status">{t("common.loading")}</div>
  {:else}
    <main class="body">
      <div class="sidebar" role="tablist" aria-orientation="vertical" aria-label={t("settings.sectionsLabel")}>
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
              <span class="dot" title={t("settings.problems", { count: section.problems })} aria-hidden="true"></span>
              <span class="sr-only">, {t("settings.problems", { count: section.problems })}</span>
            {/if}
          </button>
        {/each}
      </div>

      <div id="panel" class="content" role="tabpanel" aria-labelledby="tab-{active}" aria-busy={saving} bind:this={content}>
        <!-- Saving reloads the form from disk; edits made meanwhile would be lost. -->
        <fieldset class="panel-form" disabled={saving}>
        {#if active === "general"}
          <h1>{t("settings.sections.general")}</h1>

          <section class="group">
            <div class="row">
              <div class="label">
                <label class="name" for="language">{t("language.label")}</label>
                <span class="hint">{t("language.hint")}</span>
                {#if currentLanguage() !== "en"}
                  <span class="hint">{t("language.reviewNote")}</span>
                {/if}
              </div>
              <select id="language" class="input select" bind:value={draft.general.language}>
                <option value="auto">{t("language.auto", { language: autoLanguageName })}</option>
                {#each LANGUAGES as language (language.code)}
                  <option value={language.code} lang={language.code}>{language.name}</option>
                {/each}
              </select>
            </div>
          </section>

          <section class="group">
            <div class="field">
              <label class="name" for="hotkey">{t("settings.general.shortcut")}</label>
              {#if isWayland}
                <p class="hint">{@html tHtml("settings.general.shortcutWayland")}</p>
              {:else}
                <p class="hint">{t("settings.general.shortcutHint")}</p>
              {/if}
              <HotkeyField
                id="hotkey"
                bind:value={draft.general.hotkey}
                error={hotkeyError}
                {platform}
                presets
              />
              {#if registrationWarning}
                <p class="msg warn" role="status">
                  {t("settings.general.notRegistered", { error: registrationWarning })}
                </p>
              {/if}
              {#if hotkeyNote}
                <p class="msg info" role="status">{hotkeyNote}</p>
              {/if}
              {#if canTakeOver || canRestore}
                <div class="inline">
                  {#if canTakeOver}
                    <button
                      type="button"
                      class="btn"
                      disabled={takeoverBusy}
                      onclick={() => runTakeover(takeOverHotkey)}
                    >
                      {takeoverBusy ? t("settings.working") : takeoverLabel}
                    </button>
                  {/if}
                  {#if canRestore}
                    <button
                      type="button"
                      class="btn"
                      disabled={takeoverBusy}
                      onclick={() => runTakeover(restoreTakeover)}
                    >
                      {takeoverBusy ? t("settings.working") : restoreLabel}
                    </button>
                  {/if}
                </div>
                {#if takeoverResult}
                  <pre class="result" class:fail={!takeoverResult.ok} role="status">{takeoverResult.text}</pre>
                {/if}
              {/if}
              {#if isWayland}
                <div class="inline">
                  <button
                    type="button"
                    class="btn"
                    disabled={waylandBusy || !!hotkeyError}
                    onclick={setUpWayland}
                  >
                    {waylandBusy ? t("settings.general.settingUp") : t("settings.general.setUpGnome")}
                  </button>
                </div>
                {#if waylandResult}
                  <pre class="result" class:fail={!waylandResult.ok} role="status">{waylandResult.text}</pre>
                {/if}
              {/if}
            </div>

            <div class="field">
              <label class="name" for="actions-hotkey">{t("settings.general.actionsShortcut")}</label>
              <p class="hint">
                {t("settings.general.actionsHint")}
                {#if isWayland}
                  {@html tHtml("settings.general.actionsWayland")}
                {/if}
              </p>
              <HotkeyField
                id="actions-hotkey"
                bind:value={draft.general.actions_hotkey}
                error={actionsHotkeyError}
                {platform}
              />
              {#if actionsRegistrationWarning}
                <p class="msg warn" role="status">
                  {t("settings.general.notRegistered", { error: actionsRegistrationWarning })}
                </p>
              {/if}
            </div>

            {#if platform === "windows"}
              <div class="row">
                <div class="label">
                  <span class="name">{t("settings.general.acceptInjected")}</span>
                  <span class="hint">{t("settings.general.acceptInjectedHint")}</span>
                </div>
                <Toggle
                  bind:checked={draft.general.accept_injected_hotkeys}
                  label={t("settings.general.acceptInjected")}
                />
              </div>
            {/if}

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.general.clipboardFallback")}</span>
                <span class="hint">{t("settings.general.clipboardFallbackHint")}</span>
              </div>
              <Toggle
                bind:checked={draft.actions.use_clipboard_fallback}
                label={t("settings.general.clipboardFallback")}
              />
            </div>

            {#if display === "x11"}
              <div class="row">
                <div class="label">
                  <span class="name">{t("settings.general.primarySelection")}</span>
                  <span class="hint">{t("settings.general.primarySelectionHint")}</span>
                </div>
                <Toggle
                  bind:checked={draft.actions.use_primary_selection}
                  label={t("settings.general.primarySelectionToggle")}
                />
              </div>
            {/if}

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.general.hideOnBlur")}</span>
                <span class="hint">{t("settings.general.hideOnBlurHint")}</span>
              </div>
              <Toggle bind:checked={draft.general.hide_on_blur} label={t("settings.general.hideOnBlur")} />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.startup.name")}</span>
                <span class="hint">{t("settings.startup.hint")}</span>
                {#if platform === "windows"}
                  <span class="hint">{t("settings.startup.windowsHint")}</span>
                {:else if platform === "macos"}
                  <span class="hint">{t("settings.startup.macosHint")}</span>
                {:else}
                  <span class="hint">{t("settings.startup.linuxHint")}</span>
                {/if}
                <div class="startup-state">
                  <span class="startup-status" role="status" aria-live="polite">
                    {t("settings.startup.status", { status: startupStatusLabel })}
                  </span>
                  {#if startupError}
                    <span class="hint error" role="alert">{t("settings.startup.couldNotEnable", { error: startupError })}</span>
                  {:else if loaded?.startup.error}
                    <span class="hint error" role="alert">{t("settings.startup.couldNotCheck", { error: loaded.startup.error })}</span>
                  {/if}
                  {#if needsStartupRepair}
                    <span class="hint">{startupEnableAgain ? t("settings.startup.needsEnableAgain") : t("settings.startup.needsRepair")}</span>
                  {/if}
                  {#if needsStartupRepair && startupDraftChanged}
                    <span class="hint">{t("settings.startup.saveFirst")}</span>
                  {/if}
                  {#if startupMessage}<span class="hint" role="status">{startupMessage}</span>{/if}
                  <div class="startup-actions">
                    {#if needsStartupRepair}
                      <button type="button" class="btn" disabled={saving || startupBusy !== null || startupDraftChanged} onclick={enableStartupAgain}>
                        {startupBusy === "repair" ? t("settings.startup.enabling") : startupRepairLabel}
                      </button>
                    {/if}
                    <button type="button" class="btn" disabled={saving || startupBusy !== null} onclick={checkStartup}>
                      {startupBusy === "check" ? t("settings.startup.checking") : t("settings.startup.check")}
                    </button>
                  </div>
                </div>
              </div>
              <Toggle
                bind:checked={draft.general.launch_at_login}
                label={t("settings.startup.name")}
                disabled={saving || startupBusy !== null}
                onchange={() => { startupError = null; startupMessage = null; }}
              />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.general.updates")}</span>
                <span class="hint">{t("settings.general.updatesHint")}</span>
              </div>
              <Toggle bind:checked={draft.general.check_for_updates} label={t("settings.general.updates")} />
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="update-channel">{t("settings.general.channel")}</label>
                <span class="hint">{t("settings.general.channelHint")}</span>
              </div>
              <select
                id="update-channel"
                class="input select"
                bind:value={draft.general.update_channel}
              >
                <option value="stable">{t("settings.general.stable")}</option>
                <option value="beta">{t("settings.general.beta")}</option>
              </select>
            </div>
          </section>
        {:else if active === "hotkeys"}
          <h1>{t("settings.sections.hotkeys")}</h1>
          <HotkeyList
            bind:bindings={draft.hotkey}
            bind:parseProblems={hotkeyParseProblems}
            errors={problems.hotkeys}
            statuses={status?.custom_hotkeys ?? []}
            {platform}
            wayland={isWayland}
          />
        {:else if active === "appearance"}
          <h1>{t("settings.sections.appearance")}</h1>

          <section class="group">
            <div class="row">
              <div class="label">
                <span class="name" id="theme-label">{t("settings.appearance.theme")}</span>
                <span class="hint">{t("settings.appearance.themeHint")}</span>
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
                <label class="name" for="width">{t("settings.appearance.width")}</label>
                <span class="hint">{t("settings.appearance.widthHint")}</span>
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
                <output for="width">{t("settings.appearance.pixels", { value: draft.window.width })}</output>
              </div>
            </div>
          </section>

          <AppearanceExtras
            bind:appearance={draft.appearance}
            errors={problems.appearance}
            warnings={status?.appearance.warnings ?? []}
            blurSupported={loaded?.platform !== "linux"}
          />

          <ThemeEditor bind:appearance={draft.appearance} />
        {:else if active === "search"}
          <h1>{t("settings.sections.search")}</h1>

          <section class="group">
            <div class="row">
              <div class="label">
                <label class="name" for="max-results">{t("settings.search.maxResults")}</label>
                <span class="hint">{t("settings.search.maxResultsHint")}</span>
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
                <output for="max-results">{formatNumber(draft.search.max_results)}</output>
              </div>
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="fallback">{t("settings.search.fallback")}</label>
                <span class="hint">{t("settings.search.fallbackHint")}</span>
                {#if problems.fallback}
                  <span class="msg error" id="fallback-error" role="alert">{problems.fallback}</span>
                {/if}
              </div>
              <select
                id="fallback"
                class="input select"
                class:invalid={!!problems.fallback}
                aria-invalid={!!problems.fallback}
                aria-describedby={problems.fallback ? "fallback-error" : undefined}
                bind:value={getFallback, setFallback}
              >
                <option value="">{t("settings.search.none")}</option>
                {#if fallbackMany}
                  <option value={MANY_FALLBACKS}>{formatList(fallbackKeywords)}</option>
                {/if}
                {#each engineKeywords as engine (engine.keyword)}
                  <option value={engine.keyword}>
                    {engine.name ? `${engine.name} (${engine.keyword})` : engine.keyword}
                  </option>
                {/each}
                {#if fallbackMissing}
                  <option value={fallbackKeywords[0]}>{t("settings.search.missing", { keyword: fallbackKeywords[0] })}</option>
                {/if}
              </select>
            </div>

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.search.remember")}</span>
                <span class="hint">{t("settings.search.rememberHint")}</span>
              </div>
              <Toggle bind:checked={draft.search.query_history} label={t("settings.search.remember")} />
            </div>
          </section>
        {:else if active === "plugins"}
          <h1>{t("settings.sections.plugins")}</h1>

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
                  label={t("settings.plugins.enable", { name: plugin.name })}
                  onchange={(on) => setPlugin(plugin.id, on)}
                />
              </div>
            {/each}

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.plugins.currency")}</span>
                <span class="hint">{t("settings.plugins.currencyHint")}</span>
              </div>
              <Toggle bind:checked={draft.calculator.currency} label={t("settings.plugins.currencyToggle")} />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.plugins.expand")}</span>
                <span class="hint">{@html tHtml("settings.plugins.expandHint")}</span>
                {#if status?.snippet_expansion.problem}
                  <span class="msg warn" role="status">{status.snippet_expansion.problem}</span>
                {/if}
              </div>
              <Toggle
                bind:checked={draft.snippets.auto_expand}
                label={t("settings.plugins.expand")}
              />
            </div>

            {#if draft.snippets.auto_expand}
              <div class="row">
                <div class="label">
                  <label class="name" for="snippets-prefix">{t("settings.plugins.prefix")}</label>
                  <span class="hint">{@html tHtml("settings.plugins.prefixHint")}</span>
                </div>
                <input
                  id="snippets-prefix"
                  class="input number"
                  type="text"
                  bind:value={draft.snippets.prefix}
                  spellcheck="false"
                  autocomplete="off"
                />
              </div>

              <div class="row">
                <div class="label">
                  <label class="name" for="snippets-expand-on">{t("settings.plugins.expandOn")}</label>
                  <span class="hint">{t("settings.plugins.expandOnHint")}</span>
                </div>
                <select
                  id="snippets-expand-on"
                  class="input select"
                  bind:value={draft.snippets.expand_on}
                >
                  <option value="immediate">{t("settings.plugins.immediate")}</option>
                  <option value="delimiter">{t("settings.plugins.delimiter")}</option>
                </select>
              </div>

              <div class="row">
                <div class="label">
                  <span class="name">{t("settings.plugins.matchCase")}</span>
                  <span class="hint">{@html tHtml("settings.plugins.matchCaseHint")}</span>
                </div>
                <Toggle bind:checked={draft.snippets.case_sensitive} label={t("settings.plugins.matchCase")} />
              </div>

              <div class="row">
                <div class="label">
                  <span class="name">{t("settings.plugins.terminals")}</span>
                  <span class="hint">{t("settings.plugins.terminalsHint")}</span>
                </div>
                <Toggle
                  bind:checked={draft.snippets.expand_in_terminals}
                  label={t("settings.plugins.terminals")}
                />
              </div>

              <div class="row">
                <div class="label">
                  <span class="name">{t("settings.plugins.browsers")}</span>
                  <span class="hint">{t("settings.plugins.browsersHint")}</span>
                </div>
                <Toggle
                  bind:checked={draft.snippets.expand_in_browsers}
                  label={t("settings.plugins.browsersToggle")}
                />
              </div>

              <ListEditor
                id="snippets-ignore-apps"
                bind:items={draft.snippets.ignore_apps}
                label={t("settings.plugins.ignoreApps")}
                hint={t("settings.plugins.ignoreAppsHint")}
                placeholder={t("settings.plugins.appName")}
                emptyText={t("settings.plugins.noIgnoredApps")}
                divider
              />
            {/if}
          </section>
          <p class="note">{t("settings.plugins.note")}</p>
        {:else if active === "workflows"}
          <WorkflowsPage />
        {:else if active === "gallery"}
          <GalleryPage />
        {:else if active === "backup"}
          <BackupPage onrestored={load} />
        {:else if active === "extensions"}
          <ExtensionsPage />
        {:else if active === "help"}
          <HelpPage />
        {:else if active === "web"}
          <h1>{t("settings.sections.web")}</h1>
          <p class="note">{@html tHtml("settings.web.note")}</p>

          {#each draft.web_search as engine, i (i)}
            {@const errors = problems.engines[i] ?? {}}
            <section class="group engine" data-engine>
              <div class="engine-top">
                <div class="cell keyword">
                  <label class="mini" for="kw-{i}">{t("settings.web.keyword")}</label>
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
                    aria-describedby={errors.keyword ? `kw-${i}-error` : undefined}
                  />
                  {#if errors.keyword}<span class="msg error" id="kw-{i}-error" role="alert">{errors.keyword}</span>{/if}
                </div>
                <div class="cell name-cell">
                  <label class="mini" for="nm-{i}">{t("settings.web.name")}</label>
                  <input
                    id="nm-{i}"
                    class="input"
                    class:invalid={!!errors.name}
                    type="text"
                    bind:value={draft.web_search[i].name}
                    spellcheck="false"
                    autocomplete="off"
                    aria-invalid={!!errors.name}
                    aria-describedby={errors.name ? `nm-${i}-error` : undefined}
                  />
                  {#if errors.name}<span class="msg error" id="nm-{i}-error" role="alert">{errors.name}</span>{/if}
                </div>
                <button
                  type="button"
                  class="btn icon"
                  aria-label={t("settings.web.remove", { name: engine.name || engine.keyword || t("settings.web.engine") })}
                  title={t("common.remove")}
                  onclick={() => removeEngine(i)}>✕</button
                >
              </div>
              <div class="cell">
                <label class="mini" for="url-{i}">{t("settings.web.url")}</label>
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
                  aria-describedby={errors.url ? `url-${i}-error` : undefined}
                />
                {#if errors.url}<span class="msg error" id="url-{i}-error" role="alert">{errors.url}</span>{/if}
              </div>
            </section>
          {/each}

          {#if draft.web_search.length === 0}
            <p class="empty">{t("settings.web.empty")}</p>
          {/if}
          <div class="inline">
            <button type="button" class="btn" onclick={addEngine}>{t("settings.web.add")}</button>
          </div>
        {:else if active === "files"}
          <h1>{t("settings.sections.files")}</h1>

          <section class="group">
            <div class="field">
              <span class="name" id="dirs-label">{t("settings.files.folders")}</span>
              <p class="hint">{t("settings.files.foldersHint")}</p>
              {#if draft.files.directories.length === 0}
                <p class="empty">{t("settings.files.noFolders")}</p>
              {:else}
                <ul class="dirs" aria-labelledby="dirs-label">
                  {#each draft.files.directories as dir, i (dir)}
                    <li>
                      <span class="path" title={dir}>{dir}</span>
                      <button
                        type="button"
                        class="btn icon"
                        aria-label={t("settings.files.removeFolder", { path: dir })}
                        title={t("common.remove")}
                        onclick={() => removeDirectory(i)}>✕</button
                      >
                    </li>
                  {/each}
                </ul>
              {/if}
              <div class="inline">
                <button type="button" class="btn" onclick={addDirectory}>{t("settings.files.addFolder")}</button>
              </div>
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="depth">{t("settings.files.depth")}</label>
                <span class="hint">{t("settings.files.depthHint")}</span>
                {#if problems.filesDepth}
                  <span class="msg error" id="depth-error" role="alert">{problems.filesDepth}</span>
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
                aria-describedby={problems.filesDepth ? "depth-error" : undefined}
              />
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="files-sort">{t("settings.files.sort")}</label>
                <span class="hint">{t("settings.files.sortHint")}</span>
              </div>
              <select id="files-sort" class="input select" bind:value={draft.files.sort}>
                <option value="relevance">{t("settings.files.sortRelevance")}</option>
                <option value="name">{t("settings.files.sortName")}</option>
                <option value="modified">{t("settings.files.sortModified")}</option>
              </select>
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="files-max-results">{t("settings.files.maxResults")}</label>
                <span class="hint">{t("settings.files.maxResultsHint")}</span>
                {#if problems.filesResults}
                  <span class="msg error" id="files-results-error" role="alert">{problems.filesResults}</span>
                {/if}
              </div>
              <input
                id="files-max-results"
                class="input number"
                class:invalid={!!problems.filesResults}
                type="number"
                min="0"
                max={MAX_FILE_RESULTS}
                step="1"
                bind:value={draft.files.max_results}
                aria-invalid={!!problems.filesResults}
                aria-describedby={problems.filesResults ? "files-results-error" : undefined}
              />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.files.hidden")}</span>
                <span class="hint">{t("settings.files.hiddenHint")}</span>
              </div>
              <Toggle bind:checked={draft.files.include_hidden} label={t("settings.files.hidden")} />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.files.network")}</span>
                <span class="hint">{@html tHtml("settings.files.networkHint")}</span>
              </div>
              <Toggle bind:checked={draft.files.allow_network_paths} label={t("settings.files.network")} />
            </div>

            <div class="row">
              <div class="label">
                <label class="name" for="files-keyword">{t("settings.files.keyword")}</label>
                <span class="hint">{@html tHtml("settings.files.keywordHint")}</span>
                {#if problems.keywords["files.keyword"]}
                  <span class="msg error" id="files-keyword-error" role="alert">{problems.keywords["files.keyword"]}</span>
                {/if}
              </div>
              <input
                id="files-keyword"
                class="input number"
                class:invalid={!!problems.keywords["files.keyword"]}
                type="text"
                bind:value={draft.files.keyword}
                spellcheck="false"
                autocomplete="off"
                aria-invalid={!!problems.keywords["files.keyword"]}
                aria-describedby={problems.keywords["files.keyword"] ? "files-keyword-error" : undefined}
              />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.files.global")}</span>
                <span class="hint">{t("settings.files.globalHint")}</span>
              </div>
              <Toggle bind:checked={draft.files.global} label={t("settings.files.globalToggle")} />
            </div>

            <div class="row">
              <div class="label">
                <span class="name">{t("settings.files.wholeDisk")}</span>
                <span class="hint">
                  {t("settings.files.wholeDiskHint", {
                    indexKeyword: draft.files.index_keyword.trim() || t("settings.files.keywordPlaceholder"),
                    contentKeyword: draft.files.content_keyword.trim() || t("settings.files.keywordPlaceholder"),
                  })}
                </span>
              </div>
              <Toggle bind:checked={draft.files.use_os_index} label={t("settings.files.wholeDisk")} />
            </div>

            {#if draft.files.use_os_index}
              <div class="row">
                <div class="label">
                  <label class="name" for="files-index-keyword">{t("settings.files.indexKeyword")}</label>
                  <span class="hint">{t("settings.files.indexKeywordHint")}</span>
                  {#if problems.keywords["files.index_keyword"]}
                    <span class="msg error" id="files-index-keyword-error" role="alert">{problems.keywords["files.index_keyword"]}</span>
                  {/if}
                </div>
                <input
                  id="files-index-keyword"
                  class="input number"
                  class:invalid={!!problems.keywords["files.index_keyword"]}
                  aria-invalid={!!problems.keywords["files.index_keyword"]}
                  aria-describedby={problems.keywords["files.index_keyword"] ? "files-index-keyword-error" : undefined}
                  type="text"
                  bind:value={draft.files.index_keyword}
                  spellcheck="false"
                  autocomplete="off"
                />
              </div>

              <div class="row">
                <div class="label">
                  <label class="name" for="files-content-keyword">{t("settings.files.contentKeyword")}</label>
                  <span class="hint">{t("settings.files.contentKeywordHint")}</span>
                  {#if problems.keywords["files.content_keyword"]}
                    <span class="msg error" id="files-content-keyword-error" role="alert">{problems.keywords["files.content_keyword"]}</span>
                  {/if}
                </div>
                <input
                  id="files-content-keyword"
                  class="input number"
                  class:invalid={!!problems.keywords["files.content_keyword"]}
                  aria-invalid={!!problems.keywords["files.content_keyword"]}
                  aria-describedby={problems.keywords["files.content_keyword"] ? "files-content-keyword-error" : undefined}
                  type="text"
                  bind:value={draft.files.content_keyword}
                  spellcheck="false"
                  autocomplete="off"
                />
              </div>
            {/if}
          </section>

          <FilesExtras bind:config={draft} {problems} {pluginOff} />
        {:else if active === "clipboard"}
          <ClipboardPage
            bind:config={draft}
            {problems}
            {platform}
            pluginOff={pluginOff("clipboard")}
          />
        {:else if active === "tasks"}
          <TasksMediaPage bind:config={draft} {problems} {platform} {pluginOff} />
        {:else if active === "windows"}
          <WindowsPage bind:config={draft} {problems} {platform} {pluginOff} />
        {:else if active === "integrations"}
          <IntegrationsPage bind:config={draft} {problems} {platform} {pluginOff} />
        {:else if active === "ai"}
          <AiPage bind:config={draft} {problems} {pluginOff} />
        {:else if active === "system"}
          <SystemPage bind:config={draft} {platform} {pluginOff} />
        {:else if active === "linux"}
          <h1>{t("settings.sections.linux")}</h1>

          <section class="group">
            <div class="row">
              <div class="label">
                <span class="name">{t("settings.linux.xwayland")}</span>
                <span class="hint">{t("settings.linux.xwaylandHint")}</span>
                <span
                  class="msg"
                  class:warn={draft.linux.wayland_use_xwayland !== xwaylandAtOpen}
                  role="status"
                >
                  {t("settings.linux.restart")}
                </span>
              </div>
              <Toggle
                bind:checked={draft.linux.wayland_use_xwayland}
                label={t("settings.linux.xwayland")}
              />
            </div>
          </section>
        {/if}
        </fieldset>
      </div>
    </main>

    <footer class="footer">
      <div class="links">
        <button type="button" class="link" onclick={() => runFooter(openConfigFile)}>
          {t("settings.footer.openConfig")}
        </button>
        <button type="button" class="link" onclick={() => runFooter(openLogDir)}>
          {t("settings.footer.revealLogs")}
        </button>
      </div>
      <div class="spacer"></div>
      {#if footerError}
        <span class="foot-msg error" role="alert">{footerError}</span>
      {:else if saveError}
        <span class="foot-msg error" role="alert" title={saveError}>{saveError}</span>
      {:else if problemCount > 0}
        <span class="foot-msg error" role="status">
          {t("settings.footer.fixProblems", { count: problemCount })}
        </span>
      {/if}
      <button type="button" class="btn" disabled={saving || startupBusy !== null} onclick={cancel}>{dirty ? t("common.cancel") : t("common.close")}</button>
      <button type="button" class="btn primary" disabled={!canSave} onclick={save}>
        {saving ? t("common.saving") : t("common.save")}
      </button>
    </footer>
  {/if}

  <div class="toast" class:show={toastVisible} role="status" aria-live="polite">
    {#if toastVisible}{t("common.saved")}{/if}
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
    outline: 2px solid var(--focus-ring);
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

  .panel-form {
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
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

  .startup-state {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 10px;
  }

  .startup-status { font-size: 12.5px; font-weight: 600; }

  .startup-actions { display: flex; flex-wrap: wrap; gap: 8px; }

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

  .hint :global(code),
  .note :global(code) {
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

  .msg.info {
    margin: 6px 0 0;
    color: var(--muted);
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
    outline: 2px solid var(--focus-ring);
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
    outline: 2px solid var(--focus-ring);
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
    outline: 2px solid var(--focus-ring);
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
    outline: 2px solid var(--focus-ring);
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
    outline: 2px solid var(--focus-ring);
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
