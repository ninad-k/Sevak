<script lang="ts">
  import { pickDirectory, pickFile, type Config } from "../settings-ipc";
  import { MAX_CALENDAR_DAYS, type Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeySet from "./KeySet.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import ListEditor from "./ListEditor.svelte";
  import Row from "./Row.svelte";
  import type { KeyOption, Picker } from "./types";
  import { t, tHtml } from "../i18n.svelte";
  import "./pages.css";

  /** The Integrations page's SSH hosts, recent projects and calendar sections. */
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

  /** As in Rust's `projects::editors` (`[projects] editors`). */
  const EDITORS: KeyOption[] = $derived([
    { key: "vscode", label: "VS Code" },
    { key: "vscode-insiders", label: "VS Code Insiders" },
    { key: "vscodium", label: "VSCodium" },
    { key: "cursor", label: "Cursor" },
    { key: "jetbrains", label: t("pages.projects.jetbrains") },
  ]);

  const calendarPickers: Picker[] = $derived([
    { label: t("pages.common.addFile"), run: () => pickFile(t("pages.calendar.pickFile"), ["ics"]) },
    { label: t("pages.common.addFolder"), run: () => pickDirectory(t("pages.calendar.pickFolder")) },
  ]);
</script>

<h2 class="sp-h2">{t("pages.ssh.title")}</h2>
<p class="sp-sub">{@html tHtml("pages.ssh.sub", { keyword: config.ssh.keyword.trim() || "ssh" })}</p>
<div class="sp-group">
  <KeywordRow id="ssh-keyword" bind:value={config.ssh.keyword} error={problems.keywords["ssh.keyword"]} />
  <Row label={t("pages.ssh.knownHosts")} hint={t("pages.ssh.knownHostsHint")}>
    <Toggle bind:checked={config.ssh.known_hosts} label={t("pages.ssh.knownHosts")} />
  </Row>
</div>
<p class="sp-note privacy">{@html tHtml("pages.ssh.privacy")}</p>
{#if pluginOff("ssh")}
  <p class="sp-note warn" role="status">
    {@html tHtml("pages.common.pluginOff", { name: t("pages.ssh.plugin") })}
  </p>
{/if}

<h2 class="sp-h2">{t("pages.projects.title")}</h2>
<p class="sp-sub">{@html tHtml("pages.projects.sub", { keyword: config.projects.keyword.trim() || "code" })}</p>
<div class="sp-group">
  <KeywordRow
    id="projects-keyword"
    bind:value={config.projects.keyword}
    error={problems.keywords["projects.keyword"]}
  />
  <Row label={t("pages.projects.editors")} hint={t("pages.projects.editorsHint")}>
    <KeySet
      id="projects-editors"
      label={t("pages.projects.editors")}
      mode="only"
      options={EDITORS}
      bind:list={config.projects.editors}
    />
  </Row>
</div>
<p class="sp-note privacy">{@html tHtml("pages.projects.privacy")}</p>
{#if pluginOff("projects")}
  <p class="sp-note warn" role="status">
    {@html tHtml("pages.common.pluginOff", { name: t("pages.projects.plugin") })}
  </p>
{/if}

<h2 class="sp-h2">{t("pages.calendar.title")}</h2>
<p class="sp-sub">{@html tHtml("pages.calendar.sub", { keyword: config.calendar.keyword.trim() || "cal" })}</p>
<div class="sp-group">
  <KeywordRow
    id="calendar-keyword"
    bind:value={config.calendar.keyword}
    error={problems.keywords["calendar.keyword"]}
  />
  <ListEditor
    id="calendar-files"
    bind:items={config.calendar.files}
    label={t("pages.calendar.files")}
    hint={t("pages.calendar.filesHint")}
    placeholder="~/calendar.ics"
    emptyText={t("pages.common.noneAdded")}
    pickers={calendarPickers}
    mono
  />
  <Row
    label={t("pages.calendar.days")}
    hint={t("pages.calendar.daysHint")}
    forId="calendar-days"
    error={problems.calendarDays}
  >
    <input
      id="calendar-days"
      class="sp-input number"
      class:invalid={!!problems.calendarDays}
      type="number"
      min="1"
      max={MAX_CALENDAR_DAYS}
      step="1"
      bind:value={config.calendar.days}
      aria-invalid={!!problems.calendarDays}
    />
    <span class="sp-unit">{t("pages.common.units.days")}</span>
  </Row>
</div>
<p class="sp-note privacy">{@html tHtml("pages.calendar.privacy")}</p>
{#if pluginOff("calendar")}
  <p class="sp-note warn" role="status">
    {@html tHtml("pages.common.pluginOff", { name: t("pages.calendar.plugin") })}
  </p>
{/if}
