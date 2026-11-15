<script lang="ts">
  import { pickFile, type Config } from "../settings-ipc";
  import Toggle from "../Toggle.svelte";
  import KeySet from "./KeySet.svelte";
  import Row from "./Row.svelte";
  import { SYSTEM_KEYS } from "./keys";
  import { t, tHtml } from "../i18n.svelte";
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
  <h1>{t("pages.system.title")}</h1>
  <p class="sp-lead">{t("pages.system.lead")}</p>

  <h2 class="sp-h2">{t("pages.system.commandsTitle")}</h2>
  <p class="sp-sub">{@html tHtml("pages.system.commandsSub")}</p>
  <div class="sp-group">
    <Row label={t("pages.system.confirm")} hint={t("pages.system.confirmHint")}>
      <Toggle bind:checked={config.system.confirm} label={t("pages.system.confirm")} />
    </Row>
    <div class="sp-field">
      <span class="sp-name">{t("pages.system.offer")}</span>
      <span class="sp-hint">{t("pages.system.offerHint")}</span>
      <KeySet
        id="system-keys"
        label={t("pages.system.offer")}
        mode="hide"
        options={SYSTEM_KEYS}
        bind:list={config.system.disabled}
      />
    </div>
  </div>
  {#if platform === "macos"}
    <p class="sp-note">{@html tHtml("pages.system.noteMac")}</p>
  {:else if platform === "linux"}
    <p class="sp-note">{@html tHtml("pages.system.noteLinux")}</p>
  {/if}
  {#if pluginOff("system")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.system.commandsPlugin") })}
    </p>
  {/if}

  <h2 class="sp-h2">{t("pages.system.terminalTitle")}</h2>
  <p class="sp-sub">{@html tHtml("pages.system.terminalSub")}</p>
  <div class="sp-group">
    <Row
      label={t("pages.system.terminal")}
      hint={t("pages.system.terminalHint", { example: terminalExample })}
      forId="shell-terminal"
    >
      <input
        id="shell-terminal"
        class="sp-input wide mono"
        type="text"
        bind:value={config.shell.terminal}
        placeholder={t("pages.common.detectAutomatically")}
        spellcheck="false"
        autocomplete="off"
      />
      <button
        type="button"
        class="sp-btn"
        onclick={() => browse(t("pages.system.chooseTerminal"), (path) => (config.shell.terminal = path))}
        >{t("pages.common.browse")}</button
      >
    </Row>
    <Row
      label={t("pages.system.shell")}
      hint={platform === "macos"
        ? t("pages.system.shellHintMac")
        : t("pages.system.shellHint", { example: shellExample })}
      forId="shell-shell"
    >
      <input
        id="shell-shell"
        class="sp-input wide mono"
        type="text"
        bind:value={config.shell.shell}
        placeholder={platform === "macos" ? t("pages.system.loginShell") : t("pages.common.detectAutomatically")}
        disabled={platform === "macos"}
        spellcheck="false"
        autocomplete="off"
      />
      {#if platform !== "macos"}
        <button
          type="button"
          class="sp-btn"
          onclick={() => browse(t("pages.system.chooseShell"), (path) => (config.shell.shell = path))}
          >{t("pages.common.browse")}</button
        >
      {/if}
    </Row>
    <Row label={t("pages.system.keepOpen")} hint={t("pages.system.keepOpenHint")}>
      <Toggle bind:checked={config.shell.keep_open} label={t("pages.system.keepOpen")} />
    </Row>
  </div>
  <p class="sp-note">
    {t("pages.system.history")}
    {#if platform === "windows"}
      {t("pages.system.historyWindows")}
    {:else if platform === "linux"}
      {@html tHtml("pages.system.historyLinux")}
    {/if}
  </p>
  {#if pluginOff("shell")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.system.terminalPlugin") })}
    </p>
  {/if}
</div>
