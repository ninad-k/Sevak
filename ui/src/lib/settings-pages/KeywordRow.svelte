<script lang="ts">
  import Row from "./Row.svelte";
  import "./pages.css";

  /** A keyword field with the live clash check of `validate.ts` under it. */
  let {
    value = $bindable(),
    label = "Keyword",
    hint = "",
    error = "",
    id,
    placeholder = "",
  }: {
    value: string;
    label?: string;
    hint?: string;
    error?: string;
    id: string;
    placeholder?: string;
  } = $props();
</script>

<Row {label} {hint} forId={id} {error}>
  <input
    {id}
    class="sp-input number"
    class:invalid={!!error}
    type="text"
    bind:value
    {placeholder}
    spellcheck="false"
    autocomplete="off"
    aria-invalid={!!error}
  />
</Row>
