<script lang="ts">
  import "./pages.css";

  /**
   * A size kept in bytes in the file but edited in KB or MB. The text the user is
   * typing stays as typed ("0." on the way to "0.5"); only a change that comes
   * from outside (loading the saved config) rewrites it.
   */
  let {
    bytes = $bindable(),
    unit,
    unitLabel,
    id,
    invalid = false,
  }: {
    bytes: number;
    /** Bytes in one unit: 1024 for KB, 1048576 for MB. */
    unit: number;
    unitLabel: string;
    id: string;
    invalid?: boolean;
  } = $props();

  const parse = (text: string): number =>
    /^\d*\.?\d*$/.test(text.trim()) && /\d/.test(text) ? Math.round(Number(text) * unit) : NaN;
  const format = (value: number): string =>
    Number.isFinite(value) ? String(+(value / unit).toFixed(3)) : "";

  // svelte-ignore state_referenced_locally
  let text = $state(format(bytes));

  $effect.pre(() => {
    const current = bytes;
    // `Object.is` so that two NaNs (an unreadable entry) count as equal.
    if (!Object.is(parse(text), current)) text = format(current);
  });

  function onInput(event: Event & { currentTarget: HTMLInputElement }) {
    text = event.currentTarget.value;
    bytes = parse(text);
  }
</script>

<input
  {id}
  class="sp-input number"
  class:invalid
  type="text"
  inputmode="decimal"
  value={text}
  oninput={onInput}
  spellcheck="false"
  autocomplete="off"
  aria-invalid={invalid}
/>
<span class="sp-unit">{unitLabel}</span>
