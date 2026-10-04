<script lang="ts">
  import { pickDirectory, pickFile, type Config } from "../settings-ipc";
  import { MAX_CACHE_MINUTES, type Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import ListEditor from "./ListEditor.svelte";
  import Row from "./Row.svelte";
  import type { Picker } from "./types";
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
    /** Whether the plugin with this id is switched off under Plugins. */
    pluginOff: (id: string) => boolean;
  } = $props();

  const systemContacts = $derived(
    platform === "macos"
      ? "the Contacts app"
      : platform === "windows"
        ? "the Windows People store"
        : "Evolution's local address books",
  );

  const vcardPickers: Picker[] = [
    { label: "Add file…", run: () => pickFile("Choose a vCard file", ["vcf"]) },
    { label: "Add folder…", run: () => pickDirectory("Choose a folder of vCard files") },
  ];
</script>

<div class="sp-page">
  <h1>Integrations</h1>
  <p class="sp-lead">
    Search your contacts and 1Password logins, and look up words. Contacts and 1Password are off
    until you turn them on; everything here stays on this computer.
  </p>

  <h2 class="sp-h2">Contacts</h2>
  <p class="sp-sub">
    Type <code>{config.contacts.keyword.trim() || "c"}</code> or <code>@</code> and part of a name,
    address, number or company.
  </p>
  <div class="sp-group">
    <Row label="Search contacts" hint="Off until you turn it on.">
      <Toggle bind:checked={config.contacts.enabled} label="Search contacts" />
    </Row>
    <KeywordRow
      id="contacts-keyword"
      bind:value={config.contacts.keyword}
      error={problems.keywords["contacts.keyword"]}
      hint="The @ keyword always works too."
    />
    <Row label="Read the system address book">
      {#snippet help()}
        Also search {systemContacts}.
        {#if platform === "macos"}
          The first time you type the keyword, a row asks to allow access; until you do, only the
          vCard files below are searched.
        {:else if platform === "windows"}
          If your account’s contacts are not in it, export a vCard file instead.
        {:else}
          Other address books: export a vCard file.
        {/if}
      {/snippet}
      <Toggle bind:checked={config.contacts.use_system} label="Read the system address book" />
    </Row>
    <ListEditor
      id="contacts-vcards"
      bind:items={config.contacts.vcard_files}
      label="vCard files and folders"
      hint="Export a .vcf file from your address book, Outlook, Google Contacts or a phone. A folder adds every .vcf in it. Works everywhere without a permission."
      placeholder="~/contacts.vcf"
      emptyText="None added."
      pickers={vcardPickers}
      mono
    />
  </div>
  <p class="sp-note privacy">
    <strong>Privacy:</strong> contacts are read into memory when Sevak starts and every ten minutes.
    They are never written to disk, logged or sent anywhere, and searches here are kept out of the
    search history.
  </p>
  {#if pluginOff("contacts")}
    <p class="sp-note warn" role="status">
      “Contacts” is switched off under <strong>Plugins</strong>. Switch it on there as well.
    </p>
  {/if}

  <h2 class="sp-h2">1Password</h2>
  <p class="sp-sub">
    Find a login by title or website: <code>{config.onepassword.keyword.trim() || "1p"} github</code>.
    Sevak uses the official <code>op</code> command-line tool and never reads a password, a
    one-time code or a note.
  </p>
  <div class="sp-group">
    <Row label="Search 1Password logins" hint="Off until you turn it on.">
      <Toggle bind:checked={config.onepassword.enabled} label="Search 1Password logins" />
    </Row>
    <KeywordRow
      id="onepassword-keyword"
      bind:value={config.onepassword.keyword}
      error={problems.keywords["onepassword.keyword"]}
    />
    <Row
      label="Path to op"
      hint="Empty looks on PATH and in the usual install folders."
      forId="op-path"
    >
      <input
        id="op-path"
        class="sp-input wide mono"
        type="text"
        bind:value={config.onepassword.op_path}
        placeholder="op"
        spellcheck="false"
        autocomplete="off"
      />
      <button
        type="button"
        class="sp-btn"
        onclick={async () => {
          const picked = await pickFile("Choose the 1Password CLI (op)");
          if (picked) config.onepassword.op_path = picked;
        }}>Browse…</button
      >
    </Row>
    <Row
      label="Account"
      hint="Which account to use when several are signed in: its address (my.1password.com), short name or ID. Empty uses op’s default."
      forId="op-account"
    >
      <input
        id="op-account"
        class="sp-input wide"
        type="text"
        bind:value={config.onepassword.account}
        placeholder="my.1password.com"
        spellcheck="false"
        autocomplete="off"
      />
    </Row>
    <Row
      label="Keep the list of logins"
      hint="How long the list is kept in memory before it is refreshed. Refreshing may ask you to unlock 1Password."
      forId="op-cache"
      error={problems.cacheMinutes}
    >
      <input
        id="op-cache"
        class="sp-input number"
        class:invalid={!!problems.cacheMinutes}
        type="number"
        min="1"
        max={MAX_CACHE_MINUTES}
        step="1"
        bind:value={config.onepassword.cache_minutes}
        aria-invalid={!!problems.cacheMinutes}
      />
      <span class="sp-unit">minutes</span>
    </Row>
  </div>
  <p class="sp-note">
    <strong>Needs the 1Password CLI:</strong> install <code>op</code> and, in the 1Password app, turn
    on <em>Settings, Developer, Integrate with 1Password CLI</em>. <code>op</code> only runs when
    you type the keyword followed by a space.
  </p>
  <p class="sp-note privacy">
    <strong>Privacy:</strong> Sevak asks <code>op</code> for the list of logins (titles, vault
    names, websites and usernames) and keeps it in memory only. What you search for here is kept out
    of the search history. <code>op</code> talks to 1Password as it normally does; Sevak sends
    nothing.
  </p>
  {#if pluginOff("1password")}
    <p class="sp-note warn" role="status">
      “1Password” is switched off under <strong>Plugins</strong>. Switch it on there as well.
    </p>
  {/if}

  <h2 class="sp-h2">Dictionary and spelling</h2>
  <p class="sp-sub">
    Look up a word or fix its spelling, offline: <code>{config.dictionary.define_keyword.trim() || "define"} serendipity</code>,
    <code>{config.dictionary.spell_keyword.trim() || "spell"} recieve</code>. Words you look up are not
    saved.
  </p>
  <div class="sp-group">
    <KeywordRow
      id="define-keyword"
      label="Definitions keyword"
      bind:value={config.dictionary.define_keyword}
      error={problems.keywords["dictionary.define_keyword"]}
    />
    <KeywordRow
      id="spell-keyword"
      label="Spelling keyword"
      bind:value={config.dictionary.spell_keyword}
      error={problems.keywords["dictionary.spell_keyword"]}
    />
    <Row label="Prefer the system dictionary">
      {#snippet help()}
        {#if platform === "macos"}
          Use the macOS Dictionary for definitions.
        {:else if platform === "windows"}
          Use the Windows spell checker for spelling.
        {:else}
          Linux has no system dictionary Sevak can ask; it uses the bundled English dictionary and
          the word lists in <code>/usr/share/hunspell</code>.
        {/if}
        Off always uses the bundled English dictionary.
      {/snippet}
      <Toggle bind:checked={config.dictionary.use_system} label="Prefer the system dictionary" />
    </Row>
  </div>
  {#if pluginOff("dict")}
    <p class="sp-note warn" role="status">
      “Dictionary” is switched off under <strong>Plugins</strong>. Switch it on there as well.
    </p>
  {/if}
</div>
