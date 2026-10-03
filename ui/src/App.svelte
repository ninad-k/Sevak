<script lang="ts">
  import { onMount } from "svelte";
  import {
    getStatus,
    hideWindow,
    onHidden,
    onShow,
    onStatus,
    setContentHeight,
    type Status,
  } from "./lib/ipc";

  let query = $state("");
  let status = $state<Status | null>(null);
  let input: HTMLInputElement | undefined = $state();
  let shell: HTMLElement | undefined = $state();

  const hotkeyError = $derived(status?.hotkey.error ?? null);
  const accelerator = $derived(status?.hotkey.accelerator ?? "");

  let lastHeight = -1;

  function focusInput(select = false) {
    if (!input) return;
    input.focus();
    if (select) input.select();
  }

  function reportHeight() {
    if (!shell) return;
    const h = Math.ceil(shell.offsetHeight);
    if (h <= 0 || h === lastHeight) return;
    lastHeight = h;
    void setContentHeight(h);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      void hideWindow();
      return;
    }
    if (import.meta.env.PROD) {
      const k = e.key.toLowerCase();
      if (e.key === "F5" || ((e.ctrlKey || e.metaKey) && k === "r")) {
        e.preventDefault();
      }
    }
  }

  function onContextMenu(e: MouseEvent) {
    if (import.meta.env.PROD) e.preventDefault();
  }

  onMount(() => {
    focusInput(true);

    const resizeObserver = new ResizeObserver(reportHeight);
    if (shell) resizeObserver.observe(shell);
    reportHeight();

    getStatus().then((s) => {
      if (s) status = s;
    });

    const unlisteners = [
      onShow(() => {
        query = "";
        focusInput(true);
      }),
      onHidden(() => {
        query = "";
      }),
      onStatus((s) => {
        status = s;
      }),
    ];

    return () => {
      resizeObserver.disconnect();
      for (const u of unlisteners) void u.then((fn) => fn());
    };
  });
</script>

<svelte:window onkeydown={onKeydown} onfocus={() => focusInput()} oncontextmenu={onContextMenu} />

<main class="shell" bind:this={shell}>
  <div class="card">
    <div class="bar">
      <svg
        class="icon"
        viewBox="0 0 24 24"
        width="22"
        height="22"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <circle cx="11" cy="11" r="7" />
        <path d="m20 20-3.5-3.5" />
      </svg>
      <input
        bind:this={input}
        bind:value={query}
        type="text"
        placeholder="Type to search…"
        spellcheck="false"
        autocomplete="off"
        autocorrect="off"
        autocapitalize="off"
        aria-label="Search"
      />
    </div>

    {#if hotkeyError}
      <div class="notice" role="status">
        Shortcut "{accelerator}" is unavailable: {hotkeyError}. Edit config.toml, then choose “Reload
        index” from the tray.
      </div>
    {/if}
    <!-- Phase 2: results list goes here, below the bar and notice. -->
  </div>
</main>

<style>
  .shell {
    padding: 12px;
  }

  .card {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 56px;
    padding: 0 18px;
  }

  .icon {
    flex: none;
    color: var(--muted);
  }

  input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--fg);
    caret-color: var(--accent);
    font: inherit;
    font-size: 22px;
    padding: 0;
  }

  input::placeholder {
    color: var(--muted);
    opacity: 0.8;
  }

  .notice {
    padding: 9px 18px 11px;
    border-top: 1px solid var(--border);
    color: var(--warn);
    font-size: 12.5px;
    line-height: 1.45;
  }
</style>
