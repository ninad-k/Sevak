<script lang="ts">
  // The page of a Sevak UI window (`index.html#sevak-ui-<id>`): asks the shell
  // what to show, then renders it. Closing the window tells the shell too (the
  // window's own destroy event), so closing by the title bar also continues the
  // workflow from `closed`.
  import { t } from "../i18n.svelte";
  import SevakUi from "./SevakUi.svelte";
  import { sendUiEvent, uiSession } from "./ipc";
  import type { UiPayload } from "./types";

  let { id }: { id: string } = $props();

  let payload = $state<UiPayload | null>(null);
  let missing = $state(false);

  $effect(() => {
    void uiSession(id).then((found) => {
      payload = found;
      missing = found === null;
      if (found) document.title = found.title;
    });
  });
</script>

{#if payload}
  <SevakUi
    {payload}
    send={(event) => sendUiEvent(payload!.id, event)}
    onend={() => undefined}
  />
{:else if missing}
  <p class="gone" role="alert">{t("sevakUi.gone")}</p>
{/if}

<style>
  .gone {
    margin: 0;
    padding: 18px;
    color: var(--muted);
  }
</style>
