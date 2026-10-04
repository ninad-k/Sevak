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

  async function clear() {
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
  <h1>Clipboard &amp; paste</h1>
  <p class="sp-lead">
    Clipboard history lets you type <code>cb</code> to paste something you copied earlier: text,
    images and files. Pasting options below also apply to snippets.
  </p>

  <h2 class="sp-h2">Clipboard history</h2>
  <div class="sp-group">
    <Row
      label="Keep a clipboard history"
      hint="Off until you turn it on. Only what you copy after that is remembered."
    >
      <Toggle bind:checked={config.clipboard.enabled} label="Keep a clipboard history" />
    </Row>
  </div>
  <p class="sp-note privacy">
    <strong>Privacy:</strong> while this is on, Sevak watches the clipboard and keeps what you copy
    on this computer, in its local data folder: text and the paths of copied files in
    <code>clipboard-history.json</code>, images as PNG files.
    {#if platform === "windows"}
      {#if config.clipboard.encrypt}The files are encrypted for your Windows account.{:else}The
        files are <strong>not encrypted</strong>.{/if}
    {:else}
      The files are <strong>not encrypted</strong>; they are readable only by your user.
    {/if}
    Nothing is sent anywhere. Password managers and credential prompts are never recorded (see
    below){#if platform === "linux"}; on Linux a copy is skipped only when the app marks it as
      secret, so add other apps to Ignore apps{/if}.
  </p>
  {#if pluginOff}
    <p class="sp-note warn" role="status">
      “Clipboard history” is switched off under <strong>Plugins</strong>, so <code>cb</code> shows
      nothing. Switch it on there as well.
    </p>
  {/if}

  <h2 class="sp-h2">What to keep</h2>
  <p class="sp-sub">These apply while the history is on.</p>
  <div class="sp-group">
    <Row
      label="Number of items"
      hint="Older entries are dropped, together with their image files."
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
      label="Longest text"
      hint="Longer text is not recorded. 1 to {MAX_CLIPBOARD_ITEM_BYTES / 1024} KB."
      forId="cb-text-size"
      error={problems.clipboard.maxItemBytes}
    >
      <SizeInput
        id="cb-text-size"
        bind:bytes={config.clipboard.max_item_bytes}
        unit={1024}
        unitLabel="KB"
        invalid={!!problems.clipboard.maxItemBytes}
      />
    </Row>

    {#if platform === "windows"}
      <Row
        label="Encrypt the history"
        hint="Encrypts the history file and the images for your Windows account. Another user, another computer or a backup cannot read them."
      >
        <Toggle bind:checked={config.clipboard.encrypt} label="Encrypt the clipboard history" />
      </Row>
    {/if}

    <Row
      label="Record images"
      hint="Copied pictures, saved as PNG files next to the history."
    >
      <Toggle bind:checked={config.clipboard.images} label="Record images" />
    </Row>

    <Row
      label="Largest image"
      hint="A picture whose PNG is bigger is not recorded. Up to {MAX_CLIPBOARD_IMAGE_BYTES / 1048576} MB."
      forId="cb-image-size"
      error={problems.clipboard.maxImageBytes}
    >
      <SizeInput
        id="cb-image-size"
        bind:bytes={config.clipboard.max_image_bytes}
        unit={1048576}
        unitLabel="MB"
        invalid={!!problems.clipboard.maxImageBytes}
      />
    </Row>

    <Row
      label="Record files"
      hint="Copied files and folders: only their paths are kept; the files stay where they are."
    >
      <Toggle bind:checked={config.clipboard.files} label="Record files" />
    </Row>

    <Row
      label="Skip password managers"
      hint="Never record copies made in KeePass, 1Password, Bitwarden and similar apps, or in system credential and passphrase prompts. Add other apps below."
    >
      <Toggle
        bind:checked={config.clipboard.default_ignore_apps}
        label="Skip password managers and credential prompts"
      />
    </Row>

    <ListEditor
      id="cb-ignore"
      bind:items={config.clipboard.ignore_apps}
      label="Ignore apps"
      hint="Copies made in these apps are never recorded, such as KeePassXC or 1Password. Program or app names, any case."
      placeholder="App name"
      emptyText="No apps ignored."
    />
  </div>

  <h2 class="sp-h2">Delete what is stored</h2>
  <div class="sp-group">
    <Row
      label="Clear clipboard history"
      hint="Deletes every saved entry and the image files. This cannot be undone."
    >
      {#if phase === "confirm"}
        <button type="button" class="sp-btn danger" onclick={clear}>Delete everything</button>
        <button type="button" class="sp-btn" onclick={() => (phase = "idle")}>Cancel</button>
      {:else}
        <button
          type="button"
          class="sp-btn"
          disabled={phase === "busy"}
          onclick={() => (phase = "confirm")}
        >
          {phase === "busy" ? "Clearing…" : "Clear history…"}
        </button>
      {/if}
    </Row>
  </div>
  {#if phase === "done"}
    <p class="sp-msg ok" role="status">The clipboard history was deleted.</p>
  {:else if phase === "failed"}
    <p class="sp-msg error" role="alert">{failure}</p>
  {/if}
  <p class="sp-sub">
    This takes effect right away; it is not part of Save. You can also type <code>cb clear</code>
    in Sevak.
  </p>

  <h2 class="sp-h2">Pasting</h2>
  <div class="sp-group">
    <Row
      label="Put the clipboard back after pasting"
      hint="After pasting a clipboard entry or snippet, restore what was on the clipboard before. Off leaves the pasted text on it."
    >
      <Toggle bind:checked={config.paste.restore_clipboard} label="Restore the clipboard after pasting" />
    </Row>
  </div>
  {#if platform === "macos"}
    <p class="sp-note">
      macOS pastes only with <strong>Accessibility</strong> permission for Sevak (System Settings,
      Privacy &amp; Security). Without it, Sevak copies the entry instead.
    </p>
  {:else if platform === "linux"}
    <p class="sp-note">
      On Wayland, apps cannot be sent a paste, so Sevak copies the entry instead.
    </p>
  {:else}
    <p class="sp-note">
      Apps running as administrator cannot receive a paste; Sevak copies the entry instead.
    </p>
  {/if}
</div>
