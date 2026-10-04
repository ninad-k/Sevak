<script lang="ts">
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import { MAX_WINDOW_GAP } from "../validate";
  import KeywordRow from "../settings-pages/KeywordRow.svelte";
  import Row from "../settings-pages/Row.svelte";
  import Toggle from "../Toggle.svelte";
  import { WINDOW_COMMANDS, commandGroups, hotkeyRun, typedCommand } from "./windows-commands";
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
  <h1>Windows</h1>
  <p class="sp-lead">
    Snap, resize and move the window you were using without touching the mouse, and jump to any
    open window by its title.
  </p>

  <div class="sp-group">
    <Row
      label="Window management"
      hint="Turns the layouts and the window switcher on or off together."
    >
      <Toggle bind:checked={config.window_management.enabled} label="Window management" />
    </Row>
    <KeywordRow
      id="windows-keyword"
      label="Layouts keyword"
      bind:value={config.window_management.keyword}
      error={problems.keywords["window_management.keyword"]}
      hint="Type the keyword and a space to list the layouts, then a layout such as left. Empty removes the keyword."
    />
    <KeywordRow
      id="windows-switcher-keyword"
      label="Switcher keyword"
      bind:value={config.window_management.switcher_keyword}
      error={problems.keywords["window_management.switcher_keyword"]}
      hint="Type the keyword and part of a window’s title or app to bring that window to the front. Empty turns the switcher off."
    />
    <Row
      label="Gap"
      hint="Space in pixels between snapped windows and the edge of the screen (0 to {MAX_WINDOW_GAP}). Scaled on high-DPI displays."
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
    <Row
      label="Show in ordinary searches"
      hint="Also match layout names in plain searches such as “snap left” or “maximize”."
    >
      <Toggle
        bind:checked={config.window_management.global}
        label="Show layouts in ordinary searches"
        disabled={off}
      />
    </Row>
  </div>

  {#if platform === "windows"}
    <p class="sp-note">
      Windows that run as administrator cannot be moved by Sevak unless Sevak runs as administrator
      too. The switcher lists the windows of the current virtual desktop.
    </p>
  {:else if platform === "macos"}
    <p class="sp-note">
      Needs <strong>Accessibility</strong> access for Sevak (System Settings, Privacy &amp;
      Security, Accessibility). The first use also asks to allow controlling
      <strong>System Events</strong>. Until both are granted, the keyword explains what is missing.
    </p>
  {:else}
    <p class="sp-note">
      Works in <strong>X11</strong> sessions with a window manager that follows the EWMH standard.
      Wayland does not let applications move or list other windows, so there Sevak says so instead:
      use your desktop’s own tiling shortcuts, or log in to an X11 session.
    </p>
  {/if}
  {#if pluginOff("windows")}
    <p class="sp-note warn" role="status">
      “Window management” is switched off under <strong>Plugins</strong>. Switch it on there as
      well.
    </p>
  {/if}

  <h2 class="sp-h2">Cheat sheet</h2>
  <p class="sp-sub">
    The layouts act on the window you were in before Sevak opened. Type
    <code>{layoutsKeyword} </code> to list them, or <code>{switcherKeyword} </code> and part of a
    title to switch.
  </p>
  {#each groups as group (group.name)}
    <div class="wm-sheet" role="group" aria-label={group.name}>
      <h3 class="wm-group">{group.name}</h3>
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

  <h2 class="sp-h2">Global shortcuts</h2>
  <p class="sp-note">
    A layout can have a shortcut of its own: add a hotkey on the <strong>Hotkeys</strong> page that
    runs the result <code>{hotkeyRun(WINDOW_COMMANDS[0])}</code>
    (or <code>windows:maximize</code>, <code>windows:next_display</code> and so on; the name is the
    layout’s key). The shortcut acts on the window that has focus when you press it.
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
