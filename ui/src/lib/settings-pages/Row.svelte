<script lang="ts">
  import type { Snippet } from "svelte";
  import "./pages.css";

  /** One setting: its name and explanation on the left, its control on the right. */
  let {
    label,
    hint = "",
    help,
    forId,
    error = "",
    children,
  }: {
    label: string;
    /** Plain-text explanation under the name. */
    hint?: string;
    /** The explanation as markup (`<code>`), when plain text will not do. */
    help?: Snippet;
    /** The id of the control, so the label focuses it. */
    forId?: string;
    /** What is wrong with the value; shown under the explanation. */
    error?: string;
    children: Snippet;
  } = $props();
</script>

<div class="sp-row">
  <div class="sp-label">
    {#if forId}
      <label class="sp-name" for={forId}>{label}</label>
    {:else}
      <span class="sp-name">{label}</span>
    {/if}
    {#if hint}<span class="sp-hint">{hint}</span>{/if}
    {#if help}<span class="sp-hint">{@render help()}</span>{/if}
    {#if error}<span class="sp-msg error" role="alert">{error}</span>{/if}
  </div>
  <div class="sp-control">{@render children()}</div>
</div>
