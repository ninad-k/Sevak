<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    disabled = false,
    onchange,
  }: {
    checked?: boolean;
    /** Accessible name (the visible label lives next to the switch). */
    label: string;
    disabled?: boolean;
    onchange?: (checked: boolean) => void;
  } = $props();

  function flip() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<button
  type="button"
  class="switch"
  class:on={checked}
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={flip}
>
  <span class="knob"></span>
</button>

<style>
  .switch {
    flex: none;
    position: relative;
    width: 38px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 11px;
    background: var(--switch-off);
    cursor: pointer;
    transition: background 120ms ease;
  }

  .switch.on {
    background: var(--accent-strong);
  }

  .switch:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .switch:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
    transition: transform 120ms ease;
  }

  .on .knob {
    transform: translateX(16px);
  }

  @media (prefers-reduced-motion: reduce) {
    .switch,
    .knob {
      transition: none;
    }
  }
</style>
