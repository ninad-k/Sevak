<script lang="ts">
  import { onMount } from "svelte";
  import {
    AI_PROVIDERS,
    MAX_MAX_TOKENS,
    MAX_SYSTEM_PROMPT_CHARS,
    MAX_TIMEOUT_SECS,
    MIN_MAX_TOKENS,
    MIN_TIMEOUT_SECS,
    aiProblems,
    disclosure,
    isPlausibleKey,
    keyStatusText,
    needsKey,
    providerInfo,
    switchProvider,
    type AiProviderId,
    type KeyStatus,
  } from "../ai";
  import { aiClearKey, aiKeyStatus, aiSetKey, aiTestConnection } from "../ai-ipc";
  import type { Config } from "../settings-ipc";
  import type { Problems } from "../validate";
  import Toggle from "../Toggle.svelte";
  import KeywordRow from "./KeywordRow.svelte";
  import Row from "./Row.svelte";
  import { t, tHtml } from "../i18n.svelte";
  import "./pages.css";

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

  const provider = $derived(providerInfo(config.ai.provider));
  const fieldProblems = $derived(aiProblems(config.ai));
  const hasProblems = $derived(Object.keys(fieldProblems).length > 0);

  // The key field is write-only: what is typed goes to Rust when "Save key" is
  // pressed and is cleared from the page at once. It is never read back.
  let keyText = $state("");
  let keyStatus = $state<KeyStatus | null>(null);
  let keyBusy = $state(false);
  let keyError = $state("");

  let testing = $state(false);
  let testResult = $state<{ ok: boolean; text: string } | null>(null);

  const statusText = $derived(keyStatusText(config.ai.provider, keyStatus));
  const keyed = $derived(config.ai.provider !== "ollama");
  const keyState = $derived(
    keyed && keyStatus ? keyStatus[config.ai.provider as "openai" | "anthropic"].state : "none",
  );

  onMount(async () => {
    keyStatus = await aiKeyStatus();
  });

  function chooseProvider(event: Event & { currentTarget: HTMLSelectElement }) {
    config.ai = switchProvider(config.ai, event.currentTarget.value as AiProviderId);
    testResult = null;
    keyText = "";
    keyError = "";
  }

  async function saveKey() {
    if (!isPlausibleKey(keyText)) {
      keyError = t("pages.ai.notAKey");
      return;
    }
    keyBusy = true;
    keyError = "";
    const reply = await aiSetKey(config.ai.provider, keyText);
    keyBusy = false;
    if (reply.ok) {
      keyStatus = reply.value;
      keyText = "";
      testResult = null;
    } else {
      keyError = reply.text;
    }
  }

  async function removeKey() {
    keyBusy = true;
    keyError = "";
    const reply = await aiClearKey(config.ai.provider);
    keyBusy = false;
    if (reply.ok) keyStatus = reply.value;
    else keyError = reply.text;
  }

  async function test() {
    testing = true;
    testResult = null;
    const reply = await aiTestConnection($state.snapshot(config.ai));
    testing = false;
    testResult = reply.ok ? { ok: true, text: reply.value } : { ok: false, text: reply.text };
  }
</script>

<div class="sp-page ai-page">
  <h1>{t("pages.ai.title")}</h1>
  <p class="sp-lead">{@html tHtml("pages.ai.lead", { keyword: config.ai.keyword.trim() || "ai" })}</p>

  <p class="sp-note privacy" data-testid="ai-disclosure">
    {@html tHtml("pages.ai.whatIsSent")}
    {disclosure(config.ai)}
    {t("pages.ai.notSent")}
  </p>

  <div class="sp-group">
    <Row label={t("pages.ai.use")} hint={t("pages.common.offUntilOn")}>
      <Toggle bind:checked={config.ai.enabled} label={t("pages.ai.use")} />
    </Row>
    <KeywordRow
      id="ai-keyword"
      bind:value={config.ai.keyword}
      error={problems.keywords["ai.keyword"]}
      hint={t("pages.ai.keywordHint")}
    />
    <Row label={t("pages.ai.provider")} hint={provider.blurb} forId="ai-provider">
      <select
        id="ai-provider"
        class="sp-input provider"
        value={config.ai.provider}
        onchange={chooseProvider}
      >
        {#each AI_PROVIDERS as option (option.id)}
          <option value={option.id}>{option.label}</option>
        {/each}
      </select>
    </Row>
    <Row
      label={t("pages.ai.model")}
      hint={t("pages.ai.modelHint")}
      forId="ai-model"
      error={fieldProblems.model ?? ""}
    >
      <input
        id="ai-model"
        class="sp-input wide mono"
        class:invalid={!!fieldProblems.model}
        type="text"
        bind:value={config.ai.model}
        placeholder={provider.defaultModel}
        spellcheck="false"
        autocomplete="off"
        aria-invalid={!!fieldProblems.model}
      />
    </Row>
    <Row
      label={t("pages.ai.baseUrl")}
      hint={t("pages.ai.baseUrlHint")}
      forId="ai-base-url"
      error={fieldProblems.baseUrl ?? ""}
    >
      <input
        id="ai-base-url"
        class="sp-input wide mono"
        class:invalid={!!fieldProblems.baseUrl}
        type="text"
        bind:value={config.ai.base_url}
        placeholder={provider.defaultBaseUrl}
        spellcheck="false"
        autocomplete="off"
        aria-invalid={!!fieldProblems.baseUrl}
      />
    </Row>

    {#if keyed}
      <Row
        label={t("pages.ai.apiKey")}
        hint={needsKey(config.ai) ? t("pages.ai.apiKeyHintRequired") : t("pages.ai.apiKeyHintOptional")}
        forId="ai-key"
        error={keyError}
      >
        {#snippet help()}
          <span data-testid="ai-key-status" class:warn-text={keyState === "unreadable"}>{statusText}</span>
        {/snippet}
        <input
          id="ai-key"
          class="sp-input wide mono"
          type="password"
          bind:value={keyText}
          placeholder={keyState === "stored" ? "••••••••••••" : t("pages.ai.pasteKey")}
          spellcheck="false"
          autocomplete="off"
          aria-label={t("pages.ai.apiKeyAria")}
          aria-invalid={!!keyError}
        />
        <button
          type="button"
          class="sp-btn"
          disabled={keyBusy || keyText.trim() === ""}
          onclick={saveKey}>{t("pages.ai.saveKey")}</button
        >
        <button
          type="button"
          class="sp-btn danger"
          disabled={keyBusy || keyState !== "stored"}
          onclick={removeKey}>{t("common.remove")}</button
        >
      </Row>
    {/if}

    <div class="sp-field">
      <label class="sp-name" for="ai-system">{t("pages.ai.systemPrompt")}</label>
      <span class="sp-hint">{t("pages.ai.systemPromptHint")}</span>
      <textarea
        id="ai-system"
        class="sp-input area"
        class:invalid={!!fieldProblems.systemPrompt}
        bind:value={config.ai.system_prompt}
        rows="3"
        maxlength={MAX_SYSTEM_PROMPT_CHARS * 2}
        spellcheck="true"
        aria-invalid={!!fieldProblems.systemPrompt}
        aria-describedby={fieldProblems.systemPrompt ? "ai-system-error" : undefined}
      ></textarea>
      {#if fieldProblems.systemPrompt}
        <span class="sp-msg error" id="ai-system-error" role="alert">{fieldProblems.systemPrompt}</span>
      {/if}
    </div>

    <Row
      label={t("pages.ai.longest")}
      hint={t("pages.ai.longestHint", { min: MIN_MAX_TOKENS, max: MAX_MAX_TOKENS })}
      forId="ai-max-tokens"
      error={fieldProblems.maxTokens ?? ""}
    >
      <input
        id="ai-max-tokens"
        class="sp-input number"
        class:invalid={!!fieldProblems.maxTokens}
        type="number"
        min={MIN_MAX_TOKENS}
        max={MAX_MAX_TOKENS}
        step="1"
        bind:value={config.ai.max_tokens}
        aria-invalid={!!fieldProblems.maxTokens}
      />
      <span class="sp-unit">{t("pages.common.units.tokens")}</span>
    </Row>
    <Row
      label={t("pages.ai.timeout")}
      hint={t("pages.ai.timeoutHint", { min: MIN_TIMEOUT_SECS, max: MAX_TIMEOUT_SECS })}
      forId="ai-timeout"
      error={fieldProblems.timeout ?? ""}
    >
      <input
        id="ai-timeout"
        class="sp-input number"
        class:invalid={!!fieldProblems.timeout}
        type="number"
        min={MIN_TIMEOUT_SECS}
        max={MAX_TIMEOUT_SECS}
        step="1"
        bind:value={config.ai.timeout_secs}
        aria-invalid={!!fieldProblems.timeout}
      />
      <span class="sp-unit">{t("pages.common.units.seconds")}</span>
    </Row>

    <div class="sp-field">
      <div class="test-row">
        <button type="button" class="sp-btn" disabled={testing || hasProblems} onclick={test}>
          {testing ? t("pages.ai.testing") : t("pages.ai.test")}
        </button>
        <span class="sp-hint">{t("pages.ai.testHint")}</span>
      </div>
      {#if testResult}
        <p
          class="sp-msg"
          class:ok={testResult.ok}
          class:error={!testResult.ok}
          role="status"
          data-testid="ai-test-result"
        >
          {testResult.text}
        </p>
      {/if}
    </div>
  </div>

  <p class="sp-note">
    {@html tHtml("pages.ai.keyKept")}
    {keyStatus ? t("pages.ai.savedKeyIs", { protection: keyStatus.protection }) : ""}
    {@html tHtml("pages.ai.envVars")}
  </p>
  {#if pluginOff("ai")}
    <p class="sp-note warn" role="status">
      {@html tHtml("pages.common.pluginOff", { name: t("pages.ai.plugin") })}
    </p>
  {/if}
</div>

<style>
  .provider {
    width: 280px;
    max-width: 100%;
  }

  .area {
    display: block;
    width: 100%;
    height: auto;
    min-height: 72px;
    padding: 8px 10px;
    resize: vertical;
    line-height: 1.45;
    box-sizing: border-box;
  }

  .test-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
  }

  .warn-text {
    color: var(--warn);
  }
</style>
