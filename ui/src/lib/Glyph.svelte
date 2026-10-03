<script lang="ts">
  // Built-in result icons. Names match `IconSource::Builtin` on the Rust side;
  // unknown names fall back to the generic plugin glyph.
  let { name }: { name: string } = $props();
</script>

<svg
  viewBox="0 0 24 24"
  width="20"
  height="20"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === "app"}
    <rect x="3.5" y="3.5" width="7" height="7" rx="1.6" />
    <rect x="13.5" y="3.5" width="7" height="7" rx="1.6" />
    <rect x="3.5" y="13.5" width="7" height="7" rx="1.6" />
    <rect x="13.5" y="13.5" width="7" height="7" rx="1.6" />
  {:else if name === "calculator"}
    <rect x="5" y="2.5" width="14" height="19" rx="2.4" />
    <rect x="8" y="5.5" width="8" height="3.6" rx="0.8" />
    <path d="M8.5 13h.01M12 13h.01M15.5 13h.01M8.5 16.5h.01M12 16.5h.01M15.5 16.5h.01" />
  {:else if name === "web"}
    <circle cx="12" cy="12" r="9" />
    <path d="M3 12h18" />
    <path d="M12 3c2.6 2.7 3.9 5.7 3.9 9s-1.3 6.3-3.9 9c-2.6-2.7-3.9-5.7-3.9-9S9.4 5.7 12 3Z" />
  {:else if name === "file"}
    <path d="M14 3H7.5A2.5 2.5 0 0 0 5 5.5v13A2.5 2.5 0 0 0 7.5 21h9a2.5 2.5 0 0 0 2.5-2.5V8Z" />
    <path d="M14 3v5h5" />
    <path d="M9 13h6M9 16.5h6" />
  {:else if name === "folder"}
    <path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H10l2 2.5h6.5A2.5 2.5 0 0 1 21 10v7.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5Z" />
  {:else if name === "copy"}
    <rect x="8.5" y="8.5" width="12" height="12" rx="2.4" />
    <path d="M15.5 8.5v-1A2.5 2.5 0 0 0 13 5H6.5A2.5 2.5 0 0 0 4 7.5V14a2.5 2.5 0 0 0 2.5 2.5h2" />
  {:else if name === "terminal"}
    <rect x="3" y="4.5" width="18" height="15" rx="2.4" />
    <path d="m7.5 10 3 2.5-3 2.5M13 15h3.5" />
  {:else}
    <path d="M9 3v4M15 3v4" />
    <path d="M6 7h12v4.5a6 6 0 0 1-12 0Z" />
    <path d="M12 17.5V21" />
  {/if}
</svg>
