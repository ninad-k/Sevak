<script lang="ts">
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeySet from "./KeySet.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import Row from "./Row.svelte";
  import { TASK_KEYS } from "./keys";
  import { t, tHtml } from "../i18n.svelte";
  import "./pages.css";

  let {
    config = $bindable(),
    problems,
    platform,
    pluginOff,
  }: {
    config: Config;
    problems: Problems;
    platform: "windows" | "macos" | "linux";
    pluginOff: (id: string) => boolean;
  } = $props();
</script>

<div class="sp-page">
  <h1>{t("pages.tasks.title")}</h1>
  <p class="sp-lead">{t("pages.tasks.lead")}</p>

  <h2 class="sp-h2">{t("pages.tasks.tasksTitle")}</h2>
  <p class="sp-sub">{@html tHtml("pages.tasks.tasksSub", { keyword: config.tasks.keyword.trim() || "t" })}</p>
  <div class="sp-group">
    <Row label={t("pages.tasks.confirm")} hint={t("pages.tasks.confirmHint")}>
      <Toggle bind:checked={config.tasks.confirm} label={t("pages.tasks.confirm")} />
    </Row>
    <KeywordRow
      id="tasks-keyword"
      bind:value={config.tasks.keyword}
      error={problems.keywords["tasks.keyword"]}
      hint={t("pages.tasks.keywordHint")}
    />
    <Row label={t("pages.common.showInOrdinary")} hint={t("pages.tasks.globalHint")}>
      <Toggle bind:checked={config.tasks.global} label={t("pages.tasks.globalToggle")} />
    </Row>
    <div class="sp-field">
      <span class="sp-name">{t("pages.tasks.offer")}</span>
      <span class="sp-hint">{t("pages.tasks.offerHint")}</span>
      <KeySet
        id="tasks-keys"
        label={t("pages.tasks.offer")}
        mode="hide"
        options={TASK_KEYS}
        bind:list={config.tasks.disabled}
      />
    </div>
  </div>
  {#if platform === "windows"}
    <p class="sp-note">{@html tHtml("pages.tasks.noteWindows")}</p>
  {:else if platform === "macos"}
    <p class="sp-note">{@html tHtml("pages.tasks.noteMac")}</p>
  {:else}
    <p class="sp-note">{@html tHtml("pages.tasks.noteLinux")}</p>
  {/if}
  {#if pluginOff("tasks")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.tasks.tasksPlugin") })}
    </p>
  {/if}

  <h2 class="sp-h2">{t("pages.tasks.mediaTitle")}</h2>
  <p class="sp-sub">{@html tHtml("pages.tasks.mediaSub", { keyword: config.media.keyword.trim() || "play" })}</p>
  <div class="sp-group">
    <KeywordRow
      id="media-keyword"
      bind:value={config.media.keyword}
      error={problems.keywords["media.keyword"]}
      hint={t("pages.tasks.mediaKeywordHint")}
    />
    <Row label={t("pages.common.showInOrdinary")} hint={t("pages.tasks.mediaGlobalHint")}>
      <Toggle bind:checked={config.media.global} label={t("pages.tasks.mediaGlobalToggle")} />
    </Row>
    <Row label={t("pages.tasks.nowPlaying")} hint={t("pages.tasks.nowPlayingHint")}>
      <Toggle bind:checked={config.media.now_playing} label={t("pages.tasks.nowPlaying")} />
    </Row>
  </div>
  {#if platform === "linux"}
    <p class="sp-note">{@html tHtml("pages.tasks.mediaNoteLinux")}</p>
  {:else if platform === "macos"}
    <p class="sp-note">{t("pages.tasks.mediaNoteMac")}</p>
  {:else}
    <p class="sp-note">{t("pages.tasks.mediaNoteWindows")}</p>
  {/if}
  {#if pluginOff("media")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.tasks.mediaPlugin") })}
    </p>
  {/if}
</div>
