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
      keyError = "That does not look like an API key (no spaces or line breaks).";
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
  <h1>AI assistant</h1>
  <p class="sp-lead">
    Type <code>{config.ai.keyword.trim() || "ai"}</code> and a question in the launcher, press Enter
    and read (or copy, or paste) the answer. Off until you turn it on. Sevak never runs anything an
    answer says: it only shows it as text.
  </p>

  <p class="sp-note privacy" data-testid="ai-disclosure">
    <strong>What is sent:</strong>
    {disclosure(config.ai)}
    Nothing is sent while you type, when Sevak starts, or in the background, and there is no telemetry.
    Questions and answers are kept in memory only: not logged, not written to disk, not in your search
    history.
  </p>

  <div class="sp-group">
    <Row label="Use the AI assistant" hint="Off until you turn it on.">
      <Toggle bind:checked={config.ai.enabled} label="Use the AI assistant" />
    </Row>
    <KeywordRow
      id="ai-keyword"
      bind:value={config.ai.keyword}
      error={problems.keywords["ai.keyword"]}
      hint="Type the keyword, a space, then your question."
    />
    <Row label="Provider" hint={provider.blurb} forId="ai-provider">
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
      label="Model"
      hint="Empty uses the default shown. Providers retire models, so type the name you want."
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
      />
    </Row>
    <Row
      label="Base URL"
      hint="Empty uses the default shown. Point the OpenAI-compatible provider at another server, such as http://localhost:1234/v1 for LM Studio."
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
        label="API key"
        hint={needsKey(config.ai)
          ? "Write-only: paste it and press Save key. It is never shown again."
          : "Optional for a server on this computer. Write-only: paste it and press Save key."}
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
          placeholder={keyState === "stored" ? "••••••••••••" : "Paste an API key"}
          spellcheck="false"
          autocomplete="off"
          aria-label="API key (write-only)"
        />
        <button
          type="button"
          class="sp-btn"
          disabled={keyBusy || keyText.trim() === ""}
          onclick={saveKey}>Save key</button
        >
        <button
          type="button"
          class="sp-btn danger"
          disabled={keyBusy || keyState !== "stored"}
          onclick={removeKey}>Remove</button
        >
      </Row>
    {/if}

    <div class="sp-field">
      <label class="sp-name" for="ai-system">System prompt</label>
      <span class="sp-hint">
        Sent with every question to set the tone. Keep secrets out of it: it goes to the provider
        too.
      </span>
      <textarea
        id="ai-system"
        class="sp-input area"
        class:invalid={!!fieldProblems.systemPrompt}
        bind:value={config.ai.system_prompt}
        rows="3"
        maxlength={MAX_SYSTEM_PROMPT_CHARS * 2}
        spellcheck="true"
      ></textarea>
      {#if fieldProblems.systemPrompt}
        <span class="sp-msg error" role="alert">{fieldProblems.systemPrompt}</span>
      {/if}
    </div>

    <Row
      label="Longest answer"
      hint={`Asked of the model, in tokens (${MIN_MAX_TOKENS} to ${MAX_MAX_TOKENS}). Roughly 0.75 words each. Longer answers cost more with paid services.`}
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
      <span class="sp-unit">tokens</span>
    </Row>
    <Row
      label="Timeout"
      hint={`How long to wait for the whole answer (${MIN_TIMEOUT_SECS} to ${MAX_TIMEOUT_SECS} seconds). A local model may need time to load.`}
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
      <span class="sp-unit">seconds</span>
    </Row>

    <div class="sp-field">
      <div class="test-row">
        <button type="button" class="sp-btn" disabled={testing || hasProblems} onclick={test}>
          {testing ? "Testing…" : "Test connection"}
        </button>
        <span class="sp-hint">
          Checks the address, the key and the model with the values above (saved or not). It lists
          the provider's models and sends no question.
        </span>
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
    <strong>Where the key is kept:</strong>
    never in <code>config.toml</code>, never in a settings export or the diagnostics report, never in
    the log.
    {keyStatus ? `A saved key is ${keyStatus.protection}.` : ""}
    The <code>OPENAI_API_KEY</code> and <code>ANTHROPIC_API_KEY</code> environment variables are used
    when no key is saved here. Press Save (below) to apply the other settings.
  </p>
  {#if pluginOff("ai")}
    <p class="sp-note warn" role="status">
      “AI assistant” is switched off under <strong>Plugins</strong>. Switch it on there as well.
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
