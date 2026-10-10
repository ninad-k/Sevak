<script lang="ts">
  // "Browse the marketplace" and "Submit your extension": two buttons that open
  // a web page in the default browser (nothing is requested by Sevak itself).
  import { openMarketplace, type MarketplacePage } from "./marketplace-ipc";

  let error = $state<string | null>(null);

  async function open(page: MarketplacePage) {
    error = (await openMarketplace(page)) ?? null;
  }
</script>

<div class="market">
  <span class="text">Want more, or made your own?</span>
  <button type="button" class="link" onclick={() => open("browse")}>Browse the marketplace</button>
  <span class="dot" aria-hidden="true">·</span>
  <button type="button" class="link" onclick={() => open("submit")}>Submit your extension</button>
  <span class="text">(opens your browser)</span>
  {#if error}<span class="error" role="alert">{error}</span>{/if}
</div>

<style>
  .market {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    margin: 0 0 12px;
    font-size: 12px;
    color: var(--muted);
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-strong);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }

  .link:hover {
    text-decoration: none;
  }

  .link:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 3px;
  }

  .error {
    flex-basis: 100%;
    color: var(--error);
  }
</style>
