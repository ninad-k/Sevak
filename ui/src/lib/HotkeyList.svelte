<script lang="ts">
  import HotkeyField from "./HotkeyField.svelte";
  import type { CustomHotkeyStatus } from "./ipc";
  import { validateHotkey, type HotkeyBinding } from "./settings-ipc";
  import type { HotkeyErrors } from "./validate";

  let {
    bindings = $bindable(),
    errors,
    statuses = [],
    wayland = false,
    parseProblems = $bindable(0),
  }: {
    bindings: HotkeyBinding[];
    /** Problems the form can see on its own (missing key, duplicates, empty id). */
    errors: HotkeyErrors[];
    /** How the saved entries registered, in config order. */
    statuses?: CustomHotkeyStatus[];
    wayland?: boolean;
    /** Entries whose key Rust could not parse. */
    parseProblems?: number;
  } = $props();

  type Kind = "query" | "run";
  const kindOf = (binding: HotkeyBinding): Kind => (binding.run != null ? "run" : "query");

  function setKind(index: number, kind: Kind) {
    const old = bindings[index];
    bindings[index] =
      kind === "run" ? { key: old.key, run: old.query ?? "" } : { key: old.key, query: old.run ?? "" };
  }

  function add() {
    bindings = [...bindings, { key: "", query: "" }];
  }

  function remove(index: number) {
    bindings = bindings.filter((_, i) => i !== index);
  }

  /** The value field of entry `index`, whichever kind it is. */
  function getValue(index: number): string {
    const binding = bindings[index];
    return (kindOf(binding) === "run" ? binding.run : binding.query) ?? "";
  }

  function setValue(index: number, value: string) {
    if (kindOf(bindings[index]) === "run") bindings[index].run = value;
    else bindings[index].query = value;
  }

  // Keys are parsed by Rust (the same parser the global-shortcut plugin uses).
  let parseErrors = $state<Record<string, string | null>>({});
  let parseSeq = 0;
  $effect(() => {
    const keys = [...new Set(bindings.map((b) => b.key.trim()).filter((key) => key !== ""))];
    const mine = ++parseSeq;
    const timer = setTimeout(async () => {
      const next: Record<string, string | null> = {};
      for (const key of keys) next[key] = await validateHotkey(key);
      if (mine === parseSeq) parseErrors = next;
    }, 200);
    return () => clearTimeout(timer);
  });
  $effect(() => {
    parseProblems = bindings.filter((b) => parseErrors[b.key.trim()]).length;
  });

  /** Why a saved entry is not active; only while the entry is as it was saved. */
  function registrationError(index: number): string | null {
    const status = statuses[index];
    return status && status.key === bindings[index].key.trim() ? status.error : null;
  }
</script>

<p class="note">
  Extra global shortcuts. A shortcut can open Sevak with text already typed in, such as
  <code>g </code> or <code>&gt; </code>, or run a result directly without showing Sevak, using its
  result id such as <code>apps:firefox.desktop</code>.
  {#if wayland}
    On Wayland, run <code>sevak --setup-hotkey</code> (or use the button under General) after saving
    to bind them in GNOME.
  {/if}
</p>

{#each bindings as binding, i (i)}
  {@const rowErrors = errors[i] ?? {}}
  {@const registration = registrationError(i)}
  <section class="group entry">
    <div class="top">
      <div class="key">
        <label class="mini" for="hk-{i}">Shortcut</label>
        <HotkeyField
          id="hk-{i}"
          bind:value={binding.key}
          error={rowErrors.key ?? parseErrors[binding.key.trim()] ?? null}
        />
      </div>
      <button
        type="button"
        class="btn icon"
        aria-label="Remove shortcut {binding.key}"
        title="Remove"
        onclick={() => remove(i)}>✕</button
      >
    </div>
    <div class="what">
      <div class="cell kind">
        <label class="mini" for="hk-kind-{i}">Does</label>
        <select
          id="hk-kind-{i}"
          class="input"
          value={kindOf(binding)}
          onchange={(e) => setKind(i, e.currentTarget.value as Kind)}
        >
          <option value="query">Open Sevak with text</option>
          <option value="run">Run a result</option>
        </select>
      </div>
      <div class="cell grow">
        <label class="mini" for="hk-value-{i}">
          {kindOf(binding) === "run" ? "Result id" : "Text"}
        </label>
        <input
          id="hk-value-{i}"
          class="input mono"
          class:invalid={!!rowErrors.value}
          type="text"
          value={getValue(i)}
          oninput={(e) => setValue(i, e.currentTarget.value)}
          placeholder={kindOf(binding) === "run" ? "apps:firefox.desktop" : "> "}
          spellcheck="false"
          autocomplete="off"
          aria-invalid={!!rowErrors.value}
        />
        {#if rowErrors.value}<span class="msg error" role="alert">{rowErrors.value}</span>{/if}
      </div>
    </div>
    {#if registration}
      <p class="msg warn" role="status">Could not be registered: {registration}</p>
    {/if}
  </section>
{/each}

{#if bindings.length === 0}
  <p class="empty">No extra shortcuts.</p>
{/if}
<div class="inline">
  <button type="button" class="btn" onclick={add}>Add shortcut</button>
</div>

<style>
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

  code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    white-space: pre;
  }

  .group {
    border: 1px solid var(--border);
    border-radius: 11px;
    background: var(--surface);
  }

  .entry {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 12px;
    padding: 12px 14px 14px;
  }

  .top {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  .key {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .what {
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

  .cell.kind {
    flex: none;
  }

  .cell.grow {
    flex: 1;
  }

  .mini {
    font-size: 11.5px;
    color: var(--muted);
  }

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

  .cell.grow .input {
    width: 100%;
  }

  .input.mono {
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12.5px;
  }

  .input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .input.invalid {
    border-color: var(--error);
  }

  .msg {
    margin: 0;
    font-size: 12px;
    line-height: 1.4;
  }

  .msg.error {
    color: var(--error);
  }

  .msg.warn {
    color: var(--warn);
  }

  .inline {
    display: flex;
    gap: 8px;
    margin-top: 10px;
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

  .btn:hover {
    border-color: var(--accent-strong);
  }

  .btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .btn.icon {
    flex: none;
    width: 32px;
    margin-top: 20px;
    padding: 0;
    color: var(--muted);
  }

  .btn.icon:hover {
    color: var(--error);
    border-color: var(--error);
  }
</style>
