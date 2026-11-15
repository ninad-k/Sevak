<script lang="ts">
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import ListEditor from "./ListEditor.svelte";
  import Row from "./Row.svelte";
  import { MAX_PASSWORD_LENGTH, MAX_PASSPHRASE_WORDS, MIN_PASSWORD_LENGTH, MIN_PASSPHRASE_WORDS } from "./tidy";
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

<h2 class="sp-h2">World clock</h2>
<p class="sp-sub">
  The time anywhere, offline: <code>{timeKeyword}</code> lists your cities, <code>{timeKeyword} tokyo</code>
  shows one place and <code>{timeKeyword} 3pm pst in ist</code> converts a time, daylight saving time included.
  Enter copies the answer.
</p>
<div class="sp-group">
  <KeywordRow
    id="clock-keyword"
    bind:value={config.clock.keyword}
    error={problems.keywords["clock.keyword"]}
  />
  <ListEditor
    id="clock-cities"
    bind:items={config.clock.cities}
    label="Cities"
    hint="What a bare keyword lists, after the time here: a city (San Francisco), an abbreviation (IST) or a time-zone name (Europe/Berlin)."
    placeholder="Tokyo"
    emptyText="None added: only the time here is listed."
    divider
  />
</div>
{#if pluginOff("clock")}
  <p class="sp-note warn" role="status">
    “World clock” is switched off under <strong>Plugins</strong>. Switch it on there as well.
  </p>
{/if}

<h2 class="sp-h2">Password generator</h2>
<p class="sp-sub">
  Random passwords made on this computer: <code>{passKeyword}</code>, <code>{passKeyword} 24</code>,
  <code>{passKeyword} 6 words</code> or <code>{passKeyword} pin</code>. Enter copies one, marked so that
  clipboard histories leave it out. Nothing generated is saved or logged.
</p>
<div class="sp-group">
  <KeywordRow
    id="passwords-keyword"
    bind:value={config.passwords.keyword}
    error={problems.keywords["passwords.keyword"]}
  />
  <Row label="Password length" hint="Characters in a password ({MIN_PASSWORD_LENGTH}-{MAX_PASSWORD_LENGTH})." forId="passwords-length">
    <input
      id="passwords-length"
      class="sp-input number"
      type="number"
      min={MIN_PASSWORD_LENGTH}
      max={MAX_PASSWORD_LENGTH}
      step="1"
      bind:value={config.passwords.length}
    />
    <span class="sp-unit">characters</span>
  </Row>
  <Row label="Passphrase words" hint="Words in a passphrase ({MIN_PASSPHRASE_WORDS}-{MAX_PASSPHRASE_WORDS})." forId="passwords-words">
    <input
      id="passwords-words"
      class="sp-input number"
      type="number"
      min={MIN_PASSPHRASE_WORDS}
      max={MAX_PASSPHRASE_WORDS}
      step="1"
      bind:value={config.passwords.words}
    />
    <span class="sp-unit">words</span>
  </Row>
  <Row label="Include symbols" hint="Add symbols such as !#%+ to passwords (letters and digits are always used).">
    <Toggle bind:checked={config.passwords.symbols} label="Include symbols" />
  </Row>
</div>
{#if pluginOff("pwgen")}
  <p class="sp-note warn" role="status">
    “Password generator” is switched off under <strong>Plugins</strong>. Switch it on there as well.
  </p>
{/if}
