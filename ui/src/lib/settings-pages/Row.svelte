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

  const errorId = $derived(forId ? `${forId}-error` : undefined);

  // A screen reader reads the problem together with the control: link the
  // message to it with `aria-describedby`. The control is the caller's markup
  // (which sets its own `aria-invalid`), found by the id the label already uses.
  $effect(() => {
    if (!forId || !errorId) return;
    const control = document.getElementById(forId);
    if (!control) return;
    const ids = (control.getAttribute("aria-describedby") ?? "")
      .split(/\s+/)
      .filter((id) => id && id !== errorId);
    if (error) ids.push(errorId);
    if (ids.length > 0) control.setAttribute("aria-describedby", ids.join(" "));
    else control.removeAttribute("aria-describedby");
  });
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
    {#if error}<span class="sp-msg error" id={errorId} role="alert">{error}</span>{/if}
  </div>
  <div class="sp-control">{@render children()}</div>
</div>
