<script lang="ts">
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeySet from "./KeySet.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import Row from "./Row.svelte";
  import { BROWSER_KEYS } from "./keys";
  import { MAX_RECENT_ITEMS, MIN_RECENT_ITEMS } from "../validate";
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

  <h2 class="sp-h2">{t("pages.files.recentTitle")}</h2>
  <p class="sp-sub">{@html tHtml("pages.files.recentSub", { keyword: config.recent.keyword.trim() || "recent" })}</p>
  <div class="sp-group">
    <Row label={t("pages.files.recentEnabled")} hint={t("pages.files.recentEnabledHint")}>
      <Toggle bind:checked={config.recent.enabled} label={t("pages.files.recentEnabledToggle")} />
    </Row>
    <KeywordRow
      id="recent-keyword"
      bind:value={config.recent.keyword}
      error={problems.keywords["recent.keyword"]}
    />
    <Row
      label={t("pages.files.recentItems")}
      hint={t("pages.files.recentItemsHint")}
      forId="recent-items"
      error={problems.recentItems}
    >
      <input
        id="recent-items"
        class="sp-input number"
        class:invalid={!!problems.recentItems}
        type="number"
        min={MIN_RECENT_ITEMS}
        max={MAX_RECENT_ITEMS}
        step="1"
        bind:value={config.recent.max_items}
        aria-invalid={!!problems.recentItems}
      />
    </Row>
  </div>
  <p class="sp-note privacy">{@html tHtml("pages.files.recentPrivacy")}</p>
  {#if pluginOff("recent")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.files.recentPlugin") })}
    </p>
  {/if}
</div>
