<script lang="ts">
  import "./pages.css";
  import type { KeyOption } from "./types";
  import { t } from "../i18n.svelte";
  import { filterOptions } from "./filter";

  /**
   * A checklist over a list of keys in the config file. In `hide` mode the list
   * holds what is switched off (`[tasks] disabled`) and a checked box means
   * "offered"; in `only` mode it holds what is wanted (`[bookmarks] browsers`).
   * Entries the checklist does not know (a key from a newer Sevak, a typo) are
   * kept and shown below, so saving never drops them silently.
   */
  let {
    list = $bindable(),
    options,
    mode,
    label,
    id,
    filter = "",
  }: {
    list: string[];
    options: KeyOption[];
    mode: "hide" | "only";
    label: string;
    id: string;
    /** Shows only the options matching these words (the checked state is kept). */
    filter?: string;
  } = $props();

  const norm = (key: string) => key.trim().toLowerCase();
  const inList = (key: string) => list.some((entry) => norm(entry) === key);
  const checked = (key: string) => (mode === "hide" ? !inList(key) : inList(key));

  function set(key: string, on: boolean) {
    const listed = mode === "hide" ? !on : on;
    if (listed) {
      if (!inList(key)) list = [...list, key];
    } else {
      list = list.filter((entry) => norm(entry) !== key);
    }
  }

  const visible = $derived(filterOptions(options, filter));
  const known = $derived(new Set(options.map((option) => option.key)));
  const extras = $derived(list.filter((entry) => !known.has(norm(entry))));
</script>

<fieldset class="sp-keys" aria-labelledby="{id}-label">
  <legend class="sp-name" id="{id}-label" style="display: none">{label}</legend>
  {#each visible as option, i (option.key)}
    {#if option.group && option.group !== visible[i - 1]?.group}
      <div class="sp-key-group" style="grid-column: 1 / -1">{option.group}</div>
    {/if}
    <label class="sp-key" class:hidden={mode === "hide" && !checked(option.key)}>
      <input
        type="checkbox"
        checked={checked(option.key)}
        onchange={(e) => set(option.key, e.currentTarget.checked)}
      />
      <span>{option.label}</span>
    </label>
  {/each}
</fieldset>
{#if extras.length > 0}
  <div class="sp-extra">
    <span class="sp-hint">{t("pages.common.otherEntries")}</span>
    <div>
      {#each extras as entry (entry)}
        <span class="sp-chip">
          {entry}
          <button
            type="button"
            aria-label={t("pages.common.removeItem", { item: entry })}
            title={t("common.remove")}
            onclick={() => (list = list.filter((e) => e !== entry))}>✕</button
          >
        </span>
      {/each}
    </div>
  </div>
{/if}
