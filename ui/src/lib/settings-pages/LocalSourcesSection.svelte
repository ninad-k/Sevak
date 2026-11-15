<script lang="ts">
  import { pickDirectory, pickFile, type Config } from "../settings-ipc";
  import { MAX_CALENDAR_DAYS, type Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeySet from "./KeySet.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import ListEditor from "./ListEditor.svelte";
  import Row from "./Row.svelte";
  import type { KeyOption, Picker } from "./types";
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
  const EDITORS: KeyOption[] = [
    { key: "vscode", label: "VS Code" },
    { key: "vscode-insiders", label: "VS Code Insiders" },
    { key: "vscodium", label: "VSCodium" },
    { key: "cursor", label: "Cursor" },
    { key: "jetbrains", label: "JetBrains IDEs and Android Studio" },
  ];

  const calendarPickers: Picker[] = [
    { label: "Add file…", run: () => pickFile("Choose a calendar file", ["ics"]) },
    { label: "Add folder…", run: () => pickDirectory("Choose a folder of calendar files") },
  ];
</script>

<h2 class="sp-h2">SSH hosts</h2>
<p class="sp-sub">
  Type <code>{config.ssh.keyword.trim() || "ssh"} web</code> to connect to a host from
  <code>~/.ssh/config</code> in a terminal (the one chosen under Terminal commands).
</p>
<div class="sp-group">
  <KeywordRow id="ssh-keyword" bind:value={config.ssh.keyword} error={problems.keywords["ssh.keyword"]} />
  <Row label="Also list known hosts" hint="Hosts you connected to before (from ~/.ssh/known_hosts). Hashed entries cannot be read and are skipped.">
    <Toggle bind:checked={config.ssh.known_hosts} label="Also list known hosts" />
  </Row>
</div>
<p class="sp-note privacy">
  <strong>Privacy:</strong> only <code>~/.ssh/config</code>, the files it includes inside
  <code>~/.ssh</code> and (if on) <code>known_hosts</code> are read, never a private key. Nothing is
  sent anywhere; <code>ssh</code> runs only when you press Enter.
</p>
{#if pluginOff("ssh")}
  <p class="sp-note warn" role="status">
    “SSH hosts” is switched off under <strong>Plugins</strong>. Switch it on there as well.
  </p>
{/if}

<h2 class="sp-h2">Recent projects</h2>
<p class="sp-sub">
  Type <code>{config.projects.keyword.trim() || "code"} sevak</code> to reopen a folder or workspace in
  the editor you last used it in.
</p>
<div class="sp-group">
  <KeywordRow
    id="projects-keyword"
    bind:value={config.projects.keyword}
    error={problems.keywords["projects.keyword"]}
  />
  <Row label="Editors" hint="None ticked reads every editor that is installed.">
    <KeySet id="projects-editors" label="Editors" mode="only" options={EDITORS} bind:list={config.projects.editors} />
  </Row>
</div>
<p class="sp-note privacy">
  <strong>Privacy:</strong> the editors’ own recent lists are read from their settings folders and kept
  in memory only. Remote (SSH, WSL, container) and network-share projects are skipped.
</p>
{#if pluginOff("projects")}
  <p class="sp-note warn" role="status">
    “Recent projects” is switched off under <strong>Plugins</strong>. Switch it on there as well.
  </p>
{/if}

<h2 class="sp-h2">Calendar</h2>
<p class="sp-sub">
  Type <code>{config.calendar.keyword.trim() || "cal"}</code> for today and the next days,
  <code>{config.calendar.keyword.trim() || "cal"} tomorrow</code> or
  <code>{config.calendar.keyword.trim() || "cal"} fri</code>. Events come from .ics files on this computer
  only: no accounts, no network.
</p>
<div class="sp-group">
  <KeywordRow
    id="calendar-keyword"
    bind:value={config.calendar.keyword}
    error={problems.keywords["calendar.keyword"]}
  />
  <ListEditor
    id="calendar-files"
    bind:items={config.calendar.files}
    label="Calendar files and folders"
    hint="Export an .ics file from your calendar app, or point at a folder a sync tool fills (searched three levels deep)."
    placeholder="~/calendar.ics"
    emptyText="None added."
    pickers={calendarPickers}
    mono
  />
  <Row label="Days ahead" hint="How far ahead the agenda looks." forId="calendar-days" error={problems.calendarDays}>
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
    <span class="sp-unit">days</span>
  </Row>
</div>
<p class="sp-note privacy">
  <strong>Privacy:</strong> the files are read into memory when Sevak starts and on each refresh. Events
  are never written anywhere else, logged or sent, and are kept out of the search history.
</p>
{#if pluginOff("calendar")}
  <p class="sp-note warn" role="status">
    “Calendar” is switched off under <strong>Plugins</strong>. Switch it on there as well.
  </p>
{/if}
