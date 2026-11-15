<script lang="ts">
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeySet from "./KeySet.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import Row from "./Row.svelte";
  import { BROWSER_KEYS } from "./keys";
  import { t, tHtml } from "../i18n.svelte";
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
  <h2 class="sp-h2">{t("pages.files.bufferTitle")}</h2>
  <p class="sp-sub">{t("pages.files.bufferSub")}</p>
  <div class="sp-group">
    <Row label={t("pages.files.keepBuffer")} hint={t("pages.files.keepBufferHint")}>
      <Toggle
        bind:checked={config.file_buffer.keep_between_shows}
        label={t("pages.files.keepBufferToggle")}
      />
    </Row>
  </div>

  <h2 class="sp-h2">{t("pages.files.bookmarksTitle")}</h2>
  <p class="sp-sub">{t("pages.files.bookmarksSub")}</p>
  <div class="sp-group">
    <KeywordRow
      id="bookmarks-keyword"
      bind:value={config.bookmarks.keyword}
      error={problems.keywords["bookmarks.keyword"]}
      hint={t("pages.files.bookmarksKeywordHint")}
    />
    <Row label={t("pages.common.showInOrdinary")} hint={t("pages.files.bookmarksGlobalHint")}>
      <Toggle bind:checked={config.bookmarks.global} label={t("pages.files.bookmarksGlobalToggle")} />
    </Row>
    <div class="sp-field">
      <span class="sp-name">{t("pages.files.browsers")}</span>
      <span class="sp-hint">{t("pages.files.browsersHint")}</span>
      <KeySet
        id="bookmark-browsers"
        label={t("pages.files.browsers")}
        mode="only"
        options={BROWSER_KEYS}
        bind:list={config.bookmarks.browsers}
      />
    </div>
  </div>
  {#if pluginOff("bookmarks")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.files.bookmarksPlugin") })}
    </p>
  {/if}
</div>
