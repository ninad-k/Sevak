<script lang="ts">
  import { onDestroy } from "svelte";
  import { capture } from "./accelerator";
  import { resumeHotkey, suspendHotkey } from "./settings-ipc";

  let {
    value = $bindable(""),
    error = null,
    id,
  }: {
    value?: string;
    /** Validation message for the current value. */
    error?: string | null;
    id?: string;
  } = $props();

  let recording = $state(false);
  let preview = $state("");
  let hint = $state<string | null>(null);
  let recordButton: HTMLButtonElement | undefined = $state();

  async function start() {
    if (recording) return;
    recording = true;
    preview = "";
    hint = null;
    // The real global hotkey must not fire while the user is choosing keys.
    await suspendHotkey();
  }

  function stop() {
    if (!recording) return;
    recording = false;
    preview = "";
    void resumeHotkey();
  }

  function onKeydown(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat) return;

    const plainEscape = e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
    if (plainEscape) {
      stop();
      recordButton?.focus();
      return;
    }

    const result = capture(e);
    if (result.kind === "partial") {
      preview = result.text;
      hint = null;
    } else if (result.kind === "rejected") {
      preview = result.text ? result.text + "+" : "";
      hint = result.reason;
    } else {
      value = result.text;
      hint = null;
      stop();
      recordButton?.focus();
    }
  }

  onDestroy(stop);
</script>

<!-- Capture phase: while recording, no other handler may see the keys. -->
<svelte:window onkeydowncapture={onKeydown} />

<div class="hotkey">
  <input
    {id}
    type="text"
    bind:value
    class:invalid={!!error}
    readonly={recording}
    placeholder={recording ? "Press the shortcut…" : "e.g. Alt+Space"}
    spellcheck="false"
    autocomplete="off"
    aria-invalid={!!error}
    aria-describedby={id ? `${id}-msg` : undefined}
  />
  <button
    bind:this={recordButton}
    type="button"
    class="record"
    class:active={recording}
    onclick={() => (recording ? stop() : void start())}
    onblur={stop}
  >
    {recording ? "Press keys… (Esc to cancel)" : "Record"}
  </button>
</div>
{#if recording && preview}
  <p class="live" role="status">{preview}</p>
{/if}
{#if recording && hint}
  <p class="msg warn" role="status">{hint}</p>
{/if}
{#if error && !recording}
  <p class="msg error" id={id ? `${id}-msg` : undefined} role="alert">{error}</p>
{/if}

<style>
  .hotkey {
    display: flex;
    gap: 8px;
    align-items: stretch;
  }

  input {
    flex: 1;
    min-width: 0;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
  }

  input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  input.invalid {
    border-color: var(--error);
  }

  input[readonly] {
    color: var(--muted);
  }

  .record {
    flex: none;
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--surface);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .record:hover {
    border-color: var(--accent-strong);
  }

  .record:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .record.active {
    border-color: var(--accent-strong);
    background: var(--selected);
  }

  .live {
    margin: 6px 0 0;
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 13px;
    color: var(--accent-strong);
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
</style>
