<script lang="ts">
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeySet from "./KeySet.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import Row from "./Row.svelte";
  import { BROWSER_KEYS } from "./keys";
  import "./pages.css";

  /** The sections of the Files page that are not about the file index: the file buffer and bookmarks. */
  let {
    config = $bindable(),
    problems,
    pluginOff,
  }: {
    config: Config;
    problems: Problems;
    pluginOff: (id: string) => boolean;
  } = $props();
</script>

<div class="sp-extras">
  <h2 class="sp-h2">File buffer</h2>
  <p class="sp-sub">
    Collect several files from the results with Alt+Down, then move, copy, zip or trash them
    together.
  </p>
  <div class="sp-group">
    <Row
      label="Keep the buffer when Sevak hides"
      hint="Off empties the buffer every time the search bar closes."
    >
      <Toggle
        bind:checked={config.file_buffer.keep_between_shows}
        label="Keep the file buffer when Sevak hides"
      />
    </Row>
  </div>

  <h2 class="sp-h2">Bookmarks</h2>
  <p class="sp-sub">
    Search your browsers’ bookmarks. They are read from disk when you search; nothing is sent
    anywhere.
  </p>
  <div class="sp-group">
    <KeywordRow
      id="bookmarks-keyword"
      bind:value={config.bookmarks.keyword}
      error={problems.keywords["bookmarks.keyword"]}
      hint="Type “keyword term” to search only bookmarks. Empty removes the keyword."
    />
    <Row
      label="Show in ordinary searches"
      hint="Also list matching bookmarks for plain searches."
    >
      <Toggle bind:checked={config.bookmarks.global} label="Show bookmarks in ordinary searches" />
    </Row>
    <div class="sp-field">
      <span class="sp-name">Browsers</span>
      <span class="sp-hint">
        Tick the browsers to read, with all their profiles. None ticked reads every browser Sevak
        finds.
      </span>
      <KeySet
        id="bookmark-browsers"
        label="Browsers"
        mode="only"
        options={BROWSER_KEYS}
        bind:list={config.bookmarks.browsers}
      />
    </div>
  </div>
  {#if pluginOff("bookmarks")}
    <p class="sp-note warn" role="status">
      “Bookmarks” is switched off under <strong>Plugins</strong>. Switch it on there as well.
    </p>
  {/if}
</div>
