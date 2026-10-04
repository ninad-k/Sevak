<script lang="ts">
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeySet from "./KeySet.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import Row from "./Row.svelte";
  import { TASK_KEYS } from "./keys";
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
  <h1>Tasks &amp; media</h1>
  <p class="sp-lead">
    Ready-made actions for the operating system, and the buttons of whatever is playing.
  </p>

  <h2 class="sp-h2">Automation tasks</h2>
  <p class="sp-sub">
    Dark mode, volume, screenshots, quit or kill an app, eject a drive, keep the computer awake and
    more. Type a task’s name, or <code>{config.tasks.keyword.trim() || "t"} </code> to list them all.
  </p>
  <div class="sp-group">
    <Row
      label="Ask before risky tasks"
      hint="Confirm before force quitting an app, ending a process and restarting Explorer or Finder."
    >
      <Toggle bind:checked={config.tasks.confirm} label="Ask before risky tasks" />
    </Row>
    <KeywordRow
      id="tasks-keyword"
      bind:value={config.tasks.keyword}
      error={problems.keywords["tasks.keyword"]}
      hint="Type the keyword and a space to list the tasks. Empty removes the keyword."
    />
    <Row
      label="Show in ordinary searches"
      hint="Also match task names in plain searches such as “dark mode” or “kill chrome”."
    >
      <Toggle bind:checked={config.tasks.global} label="Show tasks in ordinary searches" />
    </Row>
    <div class="sp-field">
      <span class="sp-name">Tasks to offer</span>
      <span class="sp-hint">
        Untick a task to hide it. Only the tasks that work on this computer are offered at all.
      </span>
      <KeySet
        id="tasks-keys"
        label="Tasks to offer"
        mode="hide"
        options={TASK_KEYS}
        bind:list={config.tasks.disabled}
      />
    </div>
  </div>
  {#if platform === "windows"}
    <p class="sp-note">
      Toggling Wi-Fi or Bluetooth needs <em>Settings, Privacy &amp; security, Radios</em> to allow
      desktop apps.
    </p>
  {:else if platform === "macos"}
    <p class="sp-note">
      The first use of dark mode, volume, quit or hide-others asks for permission to control System
      Events; hide-others and show-desktop also need <strong>Accessibility</strong>.
    </p>
  {:else}
    <p class="sp-note">
      Tasks that need a helper program (<code>wmctrl</code>, <code>nmcli</code>,
      <code>udisksctl</code>…) are not listed when it is not installed.
    </p>
  {/if}
  {#if pluginOff("tasks")}
    <p class="sp-note warn" role="status">
      “Automation tasks” is switched off under <strong>Plugins</strong>. Switch it on there as well.
    </p>
  {/if}

  <h2 class="sp-h2">Media controls</h2>
  <p class="sp-sub">
    Play, pause, next and previous for whatever is playing, without switching to the player: type
    <code>pause</code> or <code>{config.media.keyword.trim() || "play"} </code>.
  </p>
  <div class="sp-group">
    <KeywordRow
      id="media-keyword"
      bind:value={config.media.keyword}
      error={problems.keywords["media.keyword"]}
      hint="Type the keyword and a space to list the buttons and the track. Empty removes the keyword."
    />
    <Row
      label="Show in ordinary searches"
      hint="Also match “pause”, “next track” and so on in plain searches."
    >
      <Toggle bind:checked={config.media.global} label="Show media controls in ordinary searches" />
    </Row>
    <Row
      label="Show what is playing"
      hint="A row with the title, artist and app. It is read from your media player when you search, and never stored or sent anywhere."
    >
      <Toggle bind:checked={config.media.now_playing} label="Show what is playing" />
    </Row>
  </div>
  {#if platform === "linux"}
    <p class="sp-note">
      Needs <strong><code>playerctl</code></strong>: the buttons and the track are not offered until
      it is installed.
    </p>
  {:else if platform === "macos"}
    <p class="sp-note">
      Buttons use the media keys. The playing track can only be read from Music and Spotify.
    </p>
  {:else}
    <p class="sp-note">
      Uses the system media session (the one in the volume flyout), else the media keys.
    </p>
  {/if}
  {#if pluginOff("media")}
    <p class="sp-note warn" role="status">
      “Media controls” is switched off under <strong>Plugins</strong>. Switch it on there as well.
    </p>
  {/if}
</div>
