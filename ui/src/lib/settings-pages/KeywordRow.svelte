<script lang="ts">
  import Row from "./Row.svelte";
  import { t } from "../i18n.svelte";
  import "./pages.css";

  /** A keyword field with the live clash check of `validate.ts` under it. */
  let {
    value = $bindable(),
    label,
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

<Row label={label ?? t("pages.common.keyword")} {hint} forId={id} {error}>
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
