<script lang="ts">
  import Toggle from "./Toggle.svelte";
  import type { Appearance } from "./settings-ipc";
  import {
    MAX_FONT_SIZE,
    MAX_OPACITY,
    MAX_RADIUS,
    MIN_FONT_SIZE,
    MIN_OPACITY,
    isColor,
    type AppearanceErrors,
  } from "./validate";

  let {
    appearance = $bindable(),
    errors,
    warnings = [],
    blurSupported = true,
  }: {
    appearance: Appearance;
    errors: AppearanceErrors;
    /** Settings Sevak ignored when it last applied the config, with the reason. */
    warnings?: string[];
    /** The platform can blur what is behind the window (not Linux). */
    blurSupported?: boolean;
  } = $props();

  /** `#rrggbb` for the native color picker, from any accepted spelling. */
  function toHex(text: string): string {
    const value = text.trim();
    if (!isColor(value)) return "#f59e0b";
    if (value.startsWith("#")) {
      return value.length === 4
        ? "#" + [...value.slice(1)].map((c) => c + c).join("")
        : value.toLowerCase();
    }
    const channels = value.match(/\d+/g) ?? [];
    return "#" + channels.map((c) => Number(c).toString(16).padStart(2, "0")).join("");
  }
</script>

<section class="group">
  <div class="row">
    <div class="label">
      <label class="name" for="accent">Accent color</label>
      <span class="hint">Highlights and the selected row. Empty keeps the theme's own.</span>
      {#if errors.accent}<span class="msg error" role="alert">{errors.accent}</span>{/if}
    </div>
    <div class="control">
      <input
        type="color"
        class="swatch"
        aria-label="Pick the accent color"
        value={toHex(appearance.accent)}
        oninput={(e) => (appearance.accent = e.currentTarget.value)}
      />
      <input
        id="accent"
        class="input mono short"
        class:invalid={!!errors.accent}
        type="text"
        bind:value={appearance.accent}
        placeholder="#7c3aed"
        spellcheck="false"
        autocomplete="off"
        aria-invalid={!!errors.accent}
      />
      <button
        type="button"
        class="btn"
        disabled={appearance.accent === ""}
        onclick={() => (appearance.accent = "")}>Reset</button
      >
    </div>
  </div>

  <div class="row">
    <div class="label">
      <label class="name" for="font-size">Font size</label>
      <span class="hint">Size of the result titles; the search bar scales with it.</span>
    </div>
    <div class="slider">
      <input
        id="font-size"
        type="range"
        min={MIN_FONT_SIZE}
        max={MAX_FONT_SIZE}
        step="1"
        bind:value={appearance.font_size}
      />
      <output for="font-size">{appearance.font_size} px</output>
    </div>
  </div>

  <div class="row">
    <div class="label">
      <label class="name" for="font-family">Font</label>
      <span class="hint">A comma-separated list, such as Fira Sans, sans-serif. Empty uses the system font.</span>
      {#if errors.fontFamily}<span class="msg error" role="alert">{errors.fontFamily}</span>{/if}
    </div>
    <input
      id="font-family"
      class="input"
      class:invalid={!!errors.fontFamily}
      type="text"
      bind:value={appearance.font_family}
      spellcheck="false"
      autocomplete="off"
      aria-invalid={!!errors.fontFamily}
    />
  </div>

  <div class="row">
    <div class="label">
      <label class="name" for="opacity">Background opacity</label>
      <span class="hint">Lower values let the desktop show through the search bar.</span>
    </div>
    <div class="slider">
      <input
        id="opacity"
        type="range"
        min={MIN_OPACITY}
        max={MAX_OPACITY}
        step="1"
        bind:value={appearance.opacity}
      />
      <output for="opacity">{appearance.opacity}%</output>
    </div>
  </div>

  {#if blurSupported}
    <div class="row">
      <div class="label">
        <span class="name">Frosted-glass blur</span>
        <span class="hint">
          Blurs the desktop behind the search bar. Lower the opacity above to see it.
        </span>
      </div>
      <Toggle bind:checked={appearance.blur} label="Frosted-glass blur" />
    </div>
  {/if}

  <div class="row">
    <div class="label">
      <label class="name" for="radius">Corner radius</label>
      <span class="hint">Roundness of the search bar.</span>
    </div>
    <div class="slider">
      <input
        id="radius"
        type="range"
        min="0"
        max={MAX_RADIUS}
        step="1"
        bind:value={appearance.radius}
      />
      <output for="radius">{appearance.radius} px</output>
    </div>
  </div>

  <div class="row">
    <div class="label">
      <label class="name" for="custom-css">Custom stylesheet</label>
      <span class="hint">
        A CSS file inside the config folder that overrides the theme variables, such as
        <code>theme.css</code>. See docs/themes.md.
      </span>
      {#if errors.customCss}<span class="msg error" role="alert">{errors.customCss}</span>{/if}
    </div>
    <input
      id="custom-css"
      class="input mono"
      class:invalid={!!errors.customCss}
      type="text"
      bind:value={appearance.custom_css}
      placeholder="theme.css"
      spellcheck="false"
      autocomplete="off"
      aria-invalid={!!errors.customCss}
    />
  </div>
</section>

{#each warnings as warning (warning)}
  <p class="msg warn" role="status">{warning}</p>
{/each}

<style>
  .group {
    margin-top: 12px;
    border: 1px solid var(--border);
    border-radius: 11px;
    background: var(--surface);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    padding: 12px 16px;
  }

  .row + .row {
    border-top: 1px solid var(--border);
  }

  .label {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .name {
    font-weight: 560;
  }

  .hint {
    font-size: 12px;
    color: var(--muted);
  }

  code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--kbd-bg);
    font-family: ui-monospace, "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
  }

  .msg {
    margin: 6px 0 0;
    font-size: 12px;
    line-height: 1.4;
  }

  .msg.error {
    margin: 0;
    color: var(--error);
  }

  .msg.warn {
    color: var(--warn);
  }

  .control {
    display: flex;
    align-items: center;
    gap: 8px;
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
    width: 200px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
  }

  .input.short {
    width: 130px;
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

  .btn {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--input-border);
    border-radius: 7px;
    background: var(--input-bg);
    color: var(--fg);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .btn:hover:not(:disabled) {
    border-color: var(--accent-strong);
  }

  .btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .slider {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .slider input[type="range"] {
    width: 190px;
    height: 20px;
    margin: 0;
    background: transparent;
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
  }

  .slider input[type="range"]::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: var(--switch-off);
  }

  .slider input[type="range"]::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border: 0;
    border-radius: 50%;
    background: var(--accent-strong);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  }

  .slider input[type="range"]:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 4px;
    border-radius: 4px;
  }

  .slider output {
    min-width: 56px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
</style>
