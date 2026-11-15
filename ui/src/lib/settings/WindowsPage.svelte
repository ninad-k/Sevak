<script lang="ts">
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import { MAX_WINDOW_GAP } from "../validate";
  import KeywordRow from "../settings-pages/KeywordRow.svelte";
  import Row from "../settings-pages/Row.svelte";
  import Toggle from "../Toggle.svelte";
  import { WINDOW_COMMANDS, commandGroups, groupName, hotkeyRun, typedCommand } from "./windows-commands";
  import { t, tHtml } from "../i18n.svelte";
  import "../settings-pages/pages.css";

  let {
    config = $bindable(),
    problems,
    platform,
    pluginOff,
  }: {
    config: Config;
    problems: Problems;
    platform: "windows" | "macos" | "linux";
    pluginOff: (id: string) => boolean;
  } = $props();

  const groups = commandGroups();
  const layoutsKeyword = $derived(config.window_management.keyword.trim() || "win");
  const switcherKeyword = $derived(config.window_management.switcher_keyword.trim() || "w");
  const off = $derived(!config.window_management.enabled);
</script>

<div class="sp-page">
  <h1>{t("pages.windows.title")}</h1>
  <p class="sp-lead">{t("pages.windows.lead")}</p>

  <div class="sp-group">
    <Row label={t("pages.windows.enable")} hint={t("pages.windows.enableHint")}>
      <Toggle bind:checked={config.window_management.enabled} label={t("pages.windows.enable")} />
    </Row>
    <KeywordRow
      id="windows-keyword"
      label={t("pages.windows.layoutsKeyword")}
      bind:value={config.window_management.keyword}
      error={problems.keywords["window_management.keyword"]}
      hint={t("pages.windows.layoutsHint")}
    />
    <KeywordRow
      id="windows-switcher-keyword"
      label={t("pages.windows.switcherKeyword")}
      bind:value={config.window_management.switcher_keyword}
      error={problems.keywords["window_management.switcher_keyword"]}
      hint={t("pages.windows.switcherHint")}
    />
    <Row
      label={t("pages.windows.gap")}
      hint={t("pages.windows.gapHint", { max: MAX_WINDOW_GAP })}
      forId="windows-gap"
      error={problems.windowGap}
    >
      <input
        id="windows-gap"
        class="sp-input number"
        class:invalid={!!problems.windowGap}
        type="number"
        min="0"
        max={MAX_WINDOW_GAP}
        step="1"
        bind:value={config.window_management.gap}
        aria-invalid={!!problems.windowGap}
        disabled={off}
      />
      <span class="sp-unit">px</span>
    </Row>
    <Row label={t("pages.common.showInOrdinary")} hint={t("pages.windows.globalHint")}>
      <Toggle
        bind:checked={config.window_management.global}
        label={t("pages.windows.globalToggle")}
        disabled={off}
      />
    </Row>
  </div>

  {#if platform === "windows"}
    <p class="sp-note">{t("pages.windows.noteWindows")}</p>
  {:else if platform === "macos"}
    <p class="sp-note">{@html tHtml("pages.windows.noteMac")}</p>
  {:else}
    <p class="sp-note">{@html tHtml("pages.windows.noteLinux")}</p>
  {/if}
  {#if pluginOff("windows")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.windows.plugin") })}
    </p>
  {/if}

  <h2 class="sp-h2">{t("pages.windows.cheatSheet")}</h2>
  <p class="sp-sub">
    {@html tHtml("pages.windows.cheatSheetSub", { layouts: layoutsKeyword, switcher: switcherKeyword })}
  </p>
  {#each groups as group (group.name)}
    <div class="wm-sheet" role="group" aria-label={groupName(group.name)}>
      <h3 class="wm-group">{groupName(group.name)}</h3>
      <dl class="wm-list">
        {#each group.commands as command (command.key)}
          <div class="wm-item">
            <dt><code>{typedCommand(config.window_management.keyword, command)}</code></dt>
            <dd>{command.label}</dd>
          </div>
        {/each}
      </dl>
    </div>
  {/each}

  <h2 class="sp-h2">{t("pages.windows.shortcutsTitle")}</h2>
  <p class="sp-note">
    {@html tHtml("pages.windows.shortcutsNote", { run: hotkeyRun(WINDOW_COMMANDS[0]) })}
  </p>
</div>

<style>
  .wm-sheet {
    margin: 0 0 12px;
  }

  .wm-group {
    margin: 10px 0 4px;
    color: var(--muted);
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
  }

  .wm-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 6px 18px;
    margin: 0;
    padding: 10px 14px;
    border: 1px solid var(--border);
    border-radius: 11px;
    background: var(--surface);
  }

  .wm-item {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .wm-item dt {
    font-size: 12.5px;
  }

  .wm-item dd {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }

  .wm-list code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
  }
</style>
