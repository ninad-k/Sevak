<script lang="ts">
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import ListEditor from "./ListEditor.svelte";
  import Row from "./Row.svelte";
  import { MAX_PASSWORD_LENGTH, MAX_PASSPHRASE_WORDS, MIN_PASSWORD_LENGTH, MIN_PASSPHRASE_WORDS } from "./tidy";
  import { t, tHtml } from "../i18n.svelte";
  import "./pages.css";

  /** The world clock (`[clock]`) and password generator (`[passwords]`) settings. */
  let {
    config = $bindable(),
    problems,
    pluginOff,
  }: {
    config: Config;
    problems: Problems;
    /** Whether the plugin with this id is switched off under Plugins. */
    pluginOff: (id: string) => boolean;
  } = $props();

  const timeKeyword = $derived(config.clock.keyword.trim() || "time");
  const passKeyword = $derived(config.passwords.keyword.trim() || "pass");
</script>

<h2 class="sp-h2">{t("pages.clock.title")}</h2>
<p class="sp-sub">{@html tHtml("pages.clock.sub", { keyword: timeKeyword })}</p>
<div class="sp-group">
  <KeywordRow
    id="clock-keyword"
    bind:value={config.clock.keyword}
    error={problems.keywords["clock.keyword"]}
  />
  <ListEditor
    id="clock-cities"
    bind:items={config.clock.cities}
    label={t("pages.clock.cities")}
    hint={t("pages.clock.citiesHint")}
    placeholder={t("pages.clock.citiesPlaceholder")}
    emptyText={t("pages.clock.citiesEmpty")}
    divider
  />
</div>
{#if pluginOff("clock")}
  <p class="sp-note warn" role="status">
    {@html tHtml("pages.common.pluginOff", { name: t("pages.clock.plugin") })}
  </p>
{/if}

<h2 class="sp-h2">{t("pages.passwords.title")}</h2>
<p class="sp-sub">{@html tHtml("pages.passwords.sub", { keyword: passKeyword })}</p>
<div class="sp-group">
  <KeywordRow
    id="passwords-keyword"
    bind:value={config.passwords.keyword}
    error={problems.keywords["passwords.keyword"]}
  />
  <Row
    label={t("pages.passwords.length")}
    hint={t("pages.passwords.lengthHint", { min: MIN_PASSWORD_LENGTH, max: MAX_PASSWORD_LENGTH })}
    forId="passwords-length"
  >
    <input
      id="passwords-length"
      class="sp-input number"
      type="number"
      min={MIN_PASSWORD_LENGTH}
      max={MAX_PASSWORD_LENGTH}
      step="1"
      bind:value={config.passwords.length}
    />
    <span class="sp-unit">{t("pages.common.units.characters")}</span>
  </Row>
  <Row
    label={t("pages.passwords.words")}
    hint={t("pages.passwords.wordsHint", { min: MIN_PASSPHRASE_WORDS, max: MAX_PASSPHRASE_WORDS })}
    forId="passwords-words"
  >
    <input
      id="passwords-words"
      class="sp-input number"
      type="number"
      min={MIN_PASSPHRASE_WORDS}
      max={MAX_PASSPHRASE_WORDS}
      step="1"
      bind:value={config.passwords.words}
    />
    <span class="sp-unit">{t("pages.common.units.words")}</span>
  </Row>
  <Row label={t("pages.passwords.symbols")} hint={t("pages.passwords.symbolsHint")}>
    <Toggle bind:checked={config.passwords.symbols} label={t("pages.passwords.symbols")} />
  </Row>
</div>
{#if pluginOff("pwgen")}
  <p class="sp-note warn" role="status">
    {@html tHtml("pages.common.pluginOff", { name: t("pages.passwords.plugin") })}
  </p>
{/if}
