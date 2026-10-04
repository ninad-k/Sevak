<script lang="ts">
  import { onDestroy } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { capture, displayAccelerator, PRESETS, type Platform } from "./accelerator";
  import {
    onHotkeyRecorded,
    resumeHotkey,
    startHotkeyRecording,
    stopHotkeyRecording,
    suspendHotkey,
  } from "./settings-ipc";

  let {
    value = $bindable(""),
    error = null,
    id,
    platform = "linux",
    presets = false,
  }: {
    value?: string;
    /** Validation message for the current value. */
    error?: string | null;
    id?: string;
    /** For showing the keys by the names this OS uses (Win, Cmd, Option). */
    platform?: Platform;
    /** Offer a dropdown of common shortcuts (Super+Space, Alt+Space...). */
    presets?: boolean;
  } = $props();

  let recording = $state(false);
  let preview = $state("");
  let hint = $state<string | null>(null);
  let recordButton: HTMLButtonElement | undefined = $state();
  /** The backend's keyboard hook is recording (Windows): it sees keys the page never does. */
  let hookRecording = false;
  let unlistenRecorded: UnlistenFn | undefined;

  /** The value as this OS names its keys, when that differs from how it is written. */
  const shownAs = $derived.by(() => {
    const text = value.trim();
    if (!text) return null;
    const shown = displayAccelerator(text, platform);
    return shown === text ? null : shown;
  });

  async function start() {
    if (recording) return;
    recording = true;
    preview = "";
    hint = null;
    // The real global hotkey must not fire while the user is choosing keys.
    await suspendHotkey();
    // Windows keeps Win+Space from the page: let the keyboard hook listen too.
    unlistenRecorded = await onHotkeyRecorded((accelerator) => {
      if (!recording) return;
      if (accelerator !== null) {
        value = accelerator;
        hint = null;
      }
      stop();
      recordButton?.focus();
    });
    hookRecording = await startHotkeyRecording();
  }

  function stop() {
    if (!recording) return;
    recording = false;
    preview = "";
    unlistenRecorded?.();
    unlistenRecorded = undefined;
    if (hookRecording) {
      hookRecording = false;
      void stopHotkeyRecording();
    }
    void resumeHotkey();
  }

  function choosePreset(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    if (select.value) value = select.value;
    select.value = "";
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
    placeholder={recording ? "Press the shortcut…" : "e.g. Super+Space"}
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
  {#if presets}
    <select class="presets" aria-label="Preset shortcuts" value="" onchange={choosePreset}>
      <option value="" disabled>Presets</option>
      {#each PRESETS as preset (preset)}
        <option value={preset}>{displayAccelerator(preset, platform)}</option>
      {/each}
    </select>
  {/if}
</div>
{#if shownAs && !recording}
  <p class="shown">Shown on this computer as <span class="mono">{shownAs}</span></p>
{/if}
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

  .presets {
    flex: none;
    height: 32px;
    max-width: 120px;
    padding: 0 8px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--surface);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .presets:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .shown {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--muted);
  }

  .shown .mono {
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
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
