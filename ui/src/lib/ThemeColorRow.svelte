<script lang="ts">
  import { formatColor, isColor, isShadow, parseColor, toHex6, withAlpha, withRgb, type ColorField } from "./themes";

  // One palette entry: a color picker, a text field that takes any spelling
  // (#rgb, #rrggbb, #rrggbbaa, rgb(), rgba()) and an opacity slider. Shadows
  // are text only.
  let {
    field,
    value,
    fallback,
    onchange,
  }: {
    field: ColorField;
    /** The theme's value, or `undefined` when the palette leaves it out. */
    value: string | undefined;
    /** What an omitted value falls back to (the Sevak theme of the same mode). */
    fallback: string;
    /** Called with the new value, or `undefined` to leave the entry out. */
    onchange: (value: string | undefined, key: string) => void;
  } = $props();

  const shown = $derived(value ?? fallback);
  // What the user is typing; kept apart so a half-typed color does not disturb the theme.
  let text = $state("");
  let focused = $state(false);
  let invalid = $state(false);

  // What this field last reported. A different `value` while the field has focus
  // came from elsewhere (undo, a variant switch) and replaces the text.
  let reported: string | undefined = undefined;

  $effect(() => {
    if (!focused || value !== reported) {
      text = value ?? "";
      invalid = false;
      reported = value;
    }
  });

  const alpha = $derived(Math.round((parseColor(shown)?.a ?? 1) * 100));
  const id = $derived(`color-${field.key}`);

  function type(next: string) {
    text = next;
    const trimmed = next.trim();
    if (trimmed === "") {
      invalid = false;
      if (field.optional) {
        reported = undefined;
        onchange(undefined, field.key);
      }
      return;
    }
    const ok = field.kind === "shadow" ? isShadow(trimmed) : isColor(trimmed);
    invalid = !ok;
    if (!ok) return;
    const color = field.kind === "color" ? parseColor(trimmed) : null;
    reported = color ? formatColor(color) : trimmed;
    onchange(reported, field.key);
  }

  function blur() {
    focused = false;
    text = value ?? "";
    invalid = false;
  }
</script>

<div class="color-row" class:invalid class:stack={field.kind === "shadow"}>
  <div class="label">
    <label class="name" for={id}>{field.label}</label>
    {#if field.hint}<span class="hint">{field.hint}</span>{/if}
  </div>
  <div class="inputs">
    {#if field.kind === "color"}
      <input
        type="color"
        class="swatch"
        aria-label="Pick: {field.label}"
        value={toHex6(shown)}
        oninput={(e) => onchange(withRgb(shown, e.currentTarget.value), field.key)}
      />
    {/if}
    <input
      {id}
      class="input mono"
      class:wide={field.kind === "shadow"}
      type="text"
      value={text}
      placeholder={value === undefined ? (field.optional ? "follows Text" : fallback) : ""}
      spellcheck="false"
      autocomplete="off"
      aria-invalid={invalid}
      aria-describedby={invalid ? `${id}-error` : undefined}
      onfocus={() => (focused = true)}
      oninput={(e) => type(e.currentTarget.value)}
      onblur={blur}
    />
    {#if field.kind === "color"}
      <input
        type="range"
        class="alpha"
        min="0"
        max="100"
        step="1"
        value={alpha}
        aria-label="Opacity of {field.label}"
        title="Opacity {alpha}%"
        oninput={(e) => onchange(withAlpha(shown, Number(e.currentTarget.value) / 100), field.key)}
      />
    {/if}
    {#if field.optional && value !== undefined}
      <button type="button" class="btn small" onclick={() => onchange(undefined, field.key)}>Clear</button>
    {/if}
  </div>
  {#if invalid}
    <span class="msg error" id="{id}-error" role="alert"
      >{field.kind === "shadow"
        ? "Use none, or e.g. 0 8px 28px rgba(0, 0, 0, 0.5)"
        : "Use #rrggbb, #rrggbbaa, rgb() or rgba()"}</span
    >
  {/if}
</div>

<style>
  .color-row {
    display: grid;
    grid-template-columns: minmax(120px, 1fr) auto;
    align-items: center;
    gap: 2px 14px;
    padding: 8px 16px;
  }

  .color-row {
    border-top: 1px solid var(--border);
  }

  .label {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .name {
    font-weight: 560;
  }

  .hint {
    font-size: 12px;
    color: var(--muted);
  }

  .inputs {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .msg {
    grid-column: 1 / -1;
    font-size: 12px;
    color: var(--error);
  }

  .swatch {
    width: 32px;
    height: 32px;
    padding: 2px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    cursor: pointer;
  }

  .input {
    width: 196px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 12.5px;
  }

  .color-row.stack {
    grid-template-columns: 1fr;
  }

  .stack .inputs {
    min-width: 0;
  }

  .input.wide {
    width: 100%;
  }

  .mono {
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
  }

  .input:focus-visible,
  .swatch:focus-visible,
  .alpha:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .invalid .input {
    border-color: var(--error);
  }

  .alpha {
    width: 56px;
    height: 20px;
    margin: 0;
    background: transparent;
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
  }

  .alpha::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: var(--switch-off);
  }

  .alpha::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 14px;
    height: 14px;
    margin-top: -5px;
    border: 0;
    border-radius: 50%;
    background: var(--accent-strong);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  }

  .btn {
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .btn.small {
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
  }

  .btn:hover {
    border-color: var(--accent-strong);
  }

  .btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  @media (max-width: 680px) {
    .color-row {
      grid-template-columns: 1fr;
    }
  }
</style>
