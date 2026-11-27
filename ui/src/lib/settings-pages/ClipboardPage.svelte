<script lang="ts">
  import { clearClipboardHistory, type Config } from "../settings-ipc";
  import {
    MAX_CLIPBOARD_ITEMS,
    MAX_CLIPBOARD_ITEM_BYTES,
    MAX_CLIPBOARD_IMAGE_BYTES,
    type Problems,
  } from "../validate";
  import Toggle from "../Toggle.svelte";
  import ListEditor from "./ListEditor.svelte";
  import Row from "./Row.svelte";
  import SizeInput from "./SizeInput.svelte";
  import { t, tHtml } from "../i18n.svelte";
  import "./pages.css";

  let {
    config = $bindable(),
    problems,
    platform,
    pluginOff,
  }: {
    config: Config;
    problems: Problems;
    platform: "windows" | "macos" | "linux";
    /** "Clipboard history" is switched off under Plugins, which hides `cb` whatever this page says. */
    pluginOff: boolean;
  } = $props();

  type Phase = "idle" | "confirm" | "busy" | "done" | "failed";
  let phase = $state<Phase>("idle");
  let failure = $state("");

  let clearButton: HTMLButtonElement | undefined = $state();
  /** The confirm buttons went away while focused: give focus back to "Clear history…". */
  let returnFocus = false;
  $effect(() => {
    if (phase === "confirm" || phase === "busy" || !clearButton || !returnFocus) return;
    returnFocus = false;
    clearButton.focus();
  });

  async function clear() {
    returnFocus = true;
    phase = "busy";
    const error = await clearClipboardHistory();
    if (error) {
      failure = error;
      phase = "failed";
    } else {
      phase = "done";
    }
  }
</script>

<div class="sp-page">
  <h1>{t("pages.clipboard.title")}</h1>
  <p class="sp-lead">{@html tHtml("pages.clipboard.lead")}</p>

  <h2 class="sp-h2">{t("pages.clipboard.historyTitle")}</h2>
  <div class="sp-group">
    <Row label={t("pages.clipboard.keep")} hint={t("pages.clipboard.keepHint")}>
      <Toggle bind:checked={config.clipboard.enabled} label={t("pages.clipboard.keep")} />
    </Row>
  </div>
  <p class="sp-note privacy">
    {@html tHtml("pages.clipboard.privacy")}
    {#if platform === "windows"}
      {#if config.clipboard.encrypt}{t("pages.clipboard.encrypted")}{:else}{@html tHtml("pages.clipboard.notEncrypted")}{/if}
    {:else}
      {@html tHtml("pages.clipboard.notEncryptedUnix")}
    {/if}
    {platform === "linux" ? t("pages.clipboard.neverRecordedLinux") : t("pages.clipboard.neverRecorded")}
  </p>
  {#if pluginOff}
    <p class="sp-note warn" role="status">{@html tHtml("pages.clipboard.pluginOff")}</p>
  {/if}

  <h2 class="sp-h2">{t("pages.clipboard.keepTitle")}</h2>
  <p class="sp-sub">{t("pages.clipboard.keepSub")}</p>
  <div class="sp-group">
    <Row
      label={t("pages.clipboard.items")}
      hint={t("pages.clipboard.itemsHint")}
      forId="cb-items"
      error={problems.clipboard.maxItems}
    >
      <input
        id="cb-items"
        class="sp-input number"
        class:invalid={!!problems.clipboard.maxItems}
        type="number"
        min="1"
        max={MAX_CLIPBOARD_ITEMS}
        step="1"
        bind:value={config.clipboard.max_items}
        aria-invalid={!!problems.clipboard.maxItems}
      />
    </Row>

    <Row
      label={t("pages.clipboard.viewerRows")}
      hint={t("pages.clipboard.viewerRowsHint")}
      forId="cb-rows"
      error={problems.clipboard.viewerRows}
    >
      <input
        id="cb-rows"
        class="sp-input number"
        class:invalid={!!problems.clipboard.viewerRows}
        type="number"
        min="5"
        max="100"
        step="1"
        bind:value={config.clipboard.viewer_rows}
        aria-invalid={!!problems.clipboard.viewerRows}
      />
    </Row>

    <Row
      label={t("pages.clipboard.previewChars")}
      hint={t("pages.clipboard.previewCharsHint")}
      forId="cb-preview"
      error={problems.clipboard.previewChars}
    >
      <input
        id="cb-preview"
        class="sp-input number"
        class:invalid={!!problems.clipboard.previewChars}
        type="number"
        min="40"
        max="500"
        step="1"
        bind:value={config.clipboard.preview_chars}
        aria-invalid={!!problems.clipboard.previewChars}
      />
    </Row>

    <Row
      label={t("pages.clipboard.longestText")}
      hint={t("pages.clipboard.longestTextHint", { max: MAX_CLIPBOARD_ITEM_BYTES / 1024 })}
      forId="cb-text-size"
      error={problems.clipboard.maxItemBytes}
    >
      <SizeInput
        id="cb-text-size"
        bind:bytes={config.clipboard.max_item_bytes}
        unit={1024}
        unitLabel={t("pages.common.units.kb")}
        invalid={!!problems.clipboard.maxItemBytes}
      />
    </Row>

    {#if platform === "windows"}
      <Row label={t("pages.clipboard.encrypt")} hint={t("pages.clipboard.encryptHint")}>
        <Toggle bind:checked={config.clipboard.encrypt} label={t("pages.clipboard.encryptToggle")} />
      </Row>
    {/if}

    <Row label={t("pages.clipboard.images")} hint={t("pages.clipboard.imagesHint")}>
      <Toggle bind:checked={config.clipboard.images} label={t("pages.clipboard.images")} />
    </Row>

    <Row
      label={t("pages.clipboard.largestImage")}
      hint={t("pages.clipboard.largestImageHint", { max: MAX_CLIPBOARD_IMAGE_BYTES / 1048576 })}
      forId="cb-image-size"
      error={problems.clipboard.maxImageBytes}
    >
      <SizeInput
        id="cb-image-size"
        bind:bytes={config.clipboard.max_image_bytes}
        unit={1048576}
        unitLabel={t("pages.common.units.mb")}
        invalid={!!problems.clipboard.maxImageBytes}
      />
    </Row>

    <Row label={t("pages.clipboard.files")} hint={t("pages.clipboard.filesHint")}>
      <Toggle bind:checked={config.clipboard.files} label={t("pages.clipboard.files")} />
    </Row>

    <Row label={t("pages.clipboard.skipManagers")} hint={t("pages.clipboard.skipManagersHint")}>
      <Toggle
        bind:checked={config.clipboard.default_ignore_apps}
        label={t("pages.clipboard.skipManagersToggle")}
      />
    </Row>

    <ListEditor
      id="cb-ignore"
      bind:items={config.clipboard.ignore_apps}
      label={t("pages.clipboard.ignore")}
      hint={t("pages.clipboard.ignoreHint")}
      placeholder={t("pages.common.appName")}
      emptyText={t("pages.common.noAppsIgnored")}
    />
  </div>

  <h2 class="sp-h2">{t("pages.clipboard.deleteTitle")}</h2>
  <div class="sp-group">
    <Row label={t("pages.clipboard.clear")} hint={t("pages.clipboard.clearHint")}>
      {#if phase === "confirm"}
        <!-- svelte-ignore a11y_autofocus -->
        <button type="button" class="sp-btn danger" onclick={clear} autofocus>{t("pages.clipboard.deleteEverything")}</button>
        <button
          type="button"
          class="sp-btn"
          onclick={() => {
            returnFocus = true;
            phase = "idle";
          }}>{t("common.cancel")}</button
        >
      {:else}
        <button
          bind:this={clearButton}
          type="button"
          class="sp-btn"
          disabled={phase === "busy"}
          onclick={() => (phase = "confirm")}
        >
          {phase === "busy" ? t("pages.clipboard.clearing") : t("pages.clipboard.clearButton")}
        </button>
      {/if}
    </Row>
  </div>
  {#if phase === "done"}
    <p class="sp-msg ok" role="status">{t("pages.clipboard.cleared")}</p>
  {:else if phase === "failed"}
    <p class="sp-msg error" role="alert">{failure}</p>
  {/if}
  <p class="sp-sub">{@html tHtml("pages.clipboard.clearNote")}</p>

  <h2 class="sp-h2">{t("pages.clipboard.pastingTitle")}</h2>
  <div class="sp-group">
    <Row label={t("pages.clipboard.restore")} hint={t("pages.clipboard.restoreHint")}>
      <Toggle bind:checked={config.paste.restore_clipboard} label={t("pages.clipboard.restoreToggle")} />
    </Row>
  </div>
  {#if platform === "macos"}
    <p class="sp-note">{@html tHtml("pages.clipboard.pasteNoteMac")}</p>
  {:else if platform === "linux"}
    <p class="sp-note">{t("pages.clipboard.pasteNoteLinux")}</p>
  {:else}
    <p class="sp-note">{t("pages.clipboard.pasteNoteWindows")}</p>
  {/if}
</div>
