<script lang="ts">
  import "./pages.css";
  import type { Picker } from "./types";

  /** A list of text entries (apps, files, ...): remove with the cross, add by typing or browsing. */
  let {
    items = $bindable(),
    label,
    hint = "",
    placeholder = "",
    emptyText = "Nothing added.",
    pickers = [],
    mono = false,
    divider = false,
    id,
  }: {
    items: string[];
    /** The accessible name of the list and the heading above it. */
    label: string;
    hint?: string;
    placeholder?: string;
    emptyText?: string;
    pickers?: Picker[];
    /** Show entries in a monospace font (paths). */
    mono?: boolean;
    /** Draw a line above it (inside a card whose rows are not `.sp-row`s). */
    divider?: boolean;
    id: string;
  } = $props();

  let draft = $state("");

  const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

  function add(value: string) {
    const entry = value.trim();
    if (entry === "") return;
    if (!items.some((item) => same(item, entry))) items = [...items, entry];
  }

  function commit() {
    add(draft);
    draft = "";
  }

  async function pick(picker: Picker) {
    const picked = await picker.run();
    if (picked) add(picked);
  }

  function remove(index: number) {
    items = items.filter((_, i) => i !== index);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    }
  }
</script>

<div class="sp-field" class:divider>
  <span class="sp-name" id="{id}-label">{label}</span>
  {#if hint}<span class="sp-hint">{hint}</span>{/if}
  {#if items.length === 0}
    <p class="sp-empty">{emptyText}</p>
  {:else}
    <ul class="sp-items" aria-labelledby="{id}-label">
      {#each items as item, i (item)}
        <li>
          <span class="sp-item-text" class:mono title={item}>{item}</span>
          <button
            type="button"
            class="sp-btn icon"
            aria-label="Remove {item}"
            title="Remove"
            onclick={() => remove(i)}>✕</button
          >
        </li>
      {/each}
    </ul>
  {/if}
  <div class="sp-add">
    <input
      {id}
      class="sp-input"
      class:mono
      type="text"
      bind:value={draft}
      {placeholder}
      spellcheck="false"
      autocomplete="off"
      aria-label="Add to {label}"
      onkeydown={onKeydown}
      onblur={commit}
    />
    <button type="button" class="sp-btn" disabled={draft.trim() === ""} onclick={commit}>Add</button>
    {#each pickers as picker (picker.label)}
      <button type="button" class="sp-btn" onclick={() => pick(picker)}>{picker.label}</button>
    {/each}
  </div>
</div>
