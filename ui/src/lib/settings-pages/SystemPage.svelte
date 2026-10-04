<script lang="ts">
  import { pickFile, type Config } from "../settings-ipc";
  import Toggle from "../Toggle.svelte";
  import KeySet from "./KeySet.svelte";
  import Row from "./Row.svelte";
  import { SYSTEM_KEYS } from "./keys";
  import "./pages.css";

  let {
    config = $bindable(),
    platform,
    pluginOff,
  }: {
    config: Config;
    platform: "windows" | "macos" | "linux";
    pluginOff: (id: string) => boolean;
  } = $props();

  const terminalExample = $derived(
    platform === "windows" ? "wt" : platform === "macos" ? "iterm" : "kitty --class sevak",
  );
  const shellExample = $derived(platform === "windows" ? "pwsh" : "zsh");

  async function browse(title: string, set: (path: string) => void) {
    const picked = await pickFile(title);
    if (picked) set(picked);
  }
</script>

<div class="sp-page">
  <h1>System &amp; terminal</h1>
  <p class="sp-lead">
    Lock, sleep, restart and shut down from the search bar, and run shell commands in a terminal.
  </p>

  <h2 class="sp-h2">System commands</h2>
  <p class="sp-sub">
    Type a command’s name, such as <code>lock</code> or <code>restart</code>, or the name of a
    settings page, such as <code>bluetooth</code>.
  </p>
  <div class="sp-group">
    <Row
      label="Ask before destructive commands"
      hint="Confirm before restart, shut down, log out and emptying the trash. This also applies to a hotkey bound to one of them."
    >
      <Toggle bind:checked={config.system.confirm} label="Ask before destructive commands" />
    </Row>
    <div class="sp-field">
      <span class="sp-name">Commands to offer</span>
      <span class="sp-hint">
        Untick a command or settings page to hide it. Only the ones that exist on this computer are
        offered at all.
      </span>
      <KeySet
        id="system-keys"
        label="Commands to offer"
        mode="hide"
        options={SYSTEM_KEYS}
        bind:list={config.system.disabled}
      />
    </div>
  </div>
  {#if platform === "macos"}
    <p class="sp-note">
      Restart, shut down, log out and emptying the Trash use System Events and need
      <strong>Accessibility</strong> permission for Sevak the first time.
    </p>
  {:else if platform === "linux"}
    <p class="sp-note">
      Settings pages open through <code>gnome-control-center</code>, so they are offered on GNOME
      only.
    </p>
  {/if}
  {#if pluginOff("system")}
    <p class="sp-note warn" role="status">
      “System commands” is switched off under <strong>Plugins</strong>. Switch it on there as well.
    </p>
  {/if}

  <h2 class="sp-h2">Terminal commands</h2>
  <p class="sp-sub">
    Type <code>&gt; git status</code> and press Enter to run it in a terminal window. Nothing runs
    until you press Enter.
  </p>
  <div class="sp-group">
    <Row
      label="Terminal"
      hint="A program name or full path, optionally with arguments, such as {terminalExample}. Empty detects one."
      forId="shell-terminal"
    >
      <input
        id="shell-terminal"
        class="sp-input wide mono"
        type="text"
        bind:value={config.shell.terminal}
        placeholder="Detect automatically"
        spellcheck="false"
        autocomplete="off"
      />
      <button
        type="button"
        class="sp-btn"
        onclick={() => browse("Choose a terminal program", (path) => (config.shell.terminal = path))}
        >Browse…</button
      >
    </Row>
    <Row
      label="Shell"
      hint={platform === "macos"
        ? "Not used on macOS: the terminal starts your login shell, so your aliases work."
        : `The program that runs the command, such as ${shellExample}. Empty detects one.`}
      forId="shell-shell"
    >
      <input
        id="shell-shell"
        class="sp-input wide mono"
        type="text"
        bind:value={config.shell.shell}
        placeholder={platform === "macos" ? "Your login shell" : "Detect automatically"}
        disabled={platform === "macos"}
        spellcheck="false"
        autocomplete="off"
      />
      {#if platform !== "macos"}
        <button
          type="button"
          class="sp-btn"
          onclick={() => browse("Choose a shell program", (path) => (config.shell.shell = path))}
          >Browse…</button
        >
      {/if}
    </Row>
    <Row
      label="Keep the terminal open"
      hint="Leave it at a shell prompt after the command exits. Off closes it when the command is done."
    >
      <Toggle bind:checked={config.shell.keep_open} label="Keep the terminal open" />
    </Row>
  </div>
  <p class="sp-note">
    Recent commands are offered again from Sevak’s usage history, which stays on this computer.
    {#if platform === "windows"}
      Commands run without your shell’s profile, so aliases and functions are not available.
    {:else if platform === "linux"}
      Commands run through the shell in non-interactive mode, so aliases from
      <code>.bashrc</code> are usually not available.
    {/if}
  </p>
  {#if pluginOff("shell")}
    <p class="sp-note warn" role="status">
      “Terminal commands” is switched off under <strong>Plugins</strong>. Switch it on there as well.
    </p>
  {/if}
</div>
