<script lang="ts">
  import { pickDirectory, pickFile, type Config } from "../settings-ipc";
  import { MAX_CACHE_MINUTES, type Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import ClockPasswordsSection from "./ClockPasswordsSection.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import LocalSourcesSection from "./LocalSourcesSection.svelte";
  import ListEditor from "./ListEditor.svelte";
  import Row from "./Row.svelte";
  import type { Picker } from "./types";
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
    /** Whether the plugin with this id is switched off under Plugins. */
    pluginOff: (id: string) => boolean;
  } = $props();

  const vcardPickers: Picker[] = $derived([
    { label: t("pages.common.addFile"), run: () => pickFile(t("pages.integrations.pickVcard"), ["vcf"]) },
    { label: t("pages.common.addFolder"), run: () => pickDirectory(t("pages.integrations.pickVcardFolder")) },
  ]);
</script>

<div class="sp-page">
  <h1>{t("pages.integrations.title")}</h1>
  <p class="sp-lead">{t("pages.integrations.lead")}</p>

  <h2 class="sp-h2">{t("pages.integrations.contactsTitle")}</h2>
  <p class="sp-sub">
    {@html tHtml("pages.integrations.contactsSub", { keyword: config.contacts.keyword.trim() || "c" })}
  </p>
  <div class="sp-group">
    <Row label={t("pages.integrations.searchContacts")} hint={t("pages.common.offUntilOn")}>
      <Toggle bind:checked={config.contacts.enabled} label={t("pages.integrations.searchContacts")} />
    </Row>
    <KeywordRow
      id="contacts-keyword"
      bind:value={config.contacts.keyword}
      error={problems.keywords["contacts.keyword"]}
      hint={t("pages.integrations.contactsKeywordHint")}
    />
    <Row
      label={t("pages.integrations.systemContacts")}
      hint={platform === "macos"
        ? t("pages.integrations.systemContactsMac")
        : platform === "windows"
          ? t("pages.integrations.systemContactsWindows")
          : t("pages.integrations.systemContactsLinux")}
    >
      <Toggle bind:checked={config.contacts.use_system} label={t("pages.integrations.systemContacts")} />
    </Row>
    <Row label={t("pages.integrations.contactPhotos")} hint={t("pages.integrations.contactPhotosHint")}>
      <Toggle bind:checked={config.contacts.show_photos} label={t("pages.integrations.contactPhotos")} />
    </Row>
    <ListEditor
      id="contacts-vcards"
      bind:items={config.contacts.vcard_files}
      label={t("pages.integrations.vcards")}
      hint={t("pages.integrations.vcardsHint")}
      placeholder="~/contacts.vcf"
      emptyText={t("pages.common.noneAdded")}
      pickers={vcardPickers}
      mono
    />
  </div>
  <p class="sp-note privacy">{@html tHtml("pages.integrations.contactsPrivacy")}</p>
  {#if pluginOff("contacts")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.integrations.contactsPlugin") })}
    </p>
  {/if}

  <h2 class="sp-h2">1Password</h2>
  <p class="sp-sub">
    {@html tHtml("pages.integrations.opSub", { keyword: config.onepassword.keyword.trim() || "1p" })}
  </p>
  <div class="sp-group">
    <Row label={t("pages.integrations.searchOp")} hint={t("pages.common.offUntilOn")}>
      <Toggle bind:checked={config.onepassword.enabled} label={t("pages.integrations.searchOp")} />
    </Row>
    <KeywordRow
      id="onepassword-keyword"
      bind:value={config.onepassword.keyword}
      error={problems.keywords["onepassword.keyword"]}
    />
    <Row
      label={t("pages.integrations.opPath")}
      hint={t("pages.integrations.opPathHint")}
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
          const picked = await pickFile(t("pages.integrations.chooseOp"));
          if (picked) config.onepassword.op_path = picked;
        }}>{t("pages.common.browse")}</button
      >
    </Row>
    <Row
      label={t("pages.integrations.account")}
      hint={t("pages.integrations.accountHint")}
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
      label={t("pages.integrations.cache")}
      hint={t("pages.integrations.cacheHint")}
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
      <span class="sp-unit">{t("pages.common.units.minutes")}</span>
    </Row>
  </div>
  <p class="sp-note">{@html tHtml("pages.integrations.opNeedsCli")}</p>
  <p class="sp-note privacy">{@html tHtml("pages.integrations.opPrivacy")}</p>
  {#if pluginOff("1password")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: "1Password" })}
    </p>
  {/if}

  <h2 class="sp-h2">{t("pages.integrations.dictionaryTitle")}</h2>
  <p class="sp-sub">
    {@html tHtml("pages.integrations.dictionarySub", {
      define: config.dictionary.define_keyword.trim() || "define",
      spell: config.dictionary.spell_keyword.trim() || "spell",
    })}
  </p>
  <div class="sp-group">
    <KeywordRow
      id="define-keyword"
      label={t("pages.integrations.defineKeyword")}
      bind:value={config.dictionary.define_keyword}
      error={problems.keywords["dictionary.define_keyword"]}
    />
    <KeywordRow
      id="spell-keyword"
      label={t("pages.integrations.spellKeyword")}
      bind:value={config.dictionary.spell_keyword}
      error={problems.keywords["dictionary.spell_keyword"]}
    />
    <Row label={t("pages.integrations.preferSystem")}>
      {#snippet help()}
        {#if platform === "macos"}
          {t("pages.integrations.preferSystemMac")}
        {:else if platform === "windows"}
          {t("pages.integrations.preferSystemWindows")}
        {:else}
          {@html tHtml("pages.integrations.preferSystemLinux")}
        {/if}
      {/snippet}
      <Toggle bind:checked={config.dictionary.use_system} label={t("pages.integrations.preferSystem")} />
    </Row>
  </div>
  {#if pluginOff("dict")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.integrations.dictionaryPlugin") })}
    </p>
  {/if}

  <LocalSourcesSection bind:config {problems} {pluginOff} />

  <ClockPasswordsSection bind:config {problems} {pluginOff} />
</div>
