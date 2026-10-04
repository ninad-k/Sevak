// The pure logic of Settings > AI assistant: providers and their defaults, what
// is sent where, and the checks Rust repeats on save (`sevak_core::ai`). Nothing
// here touches the network or the IPC layer; see ai-ipc.ts for that.

export type AiProviderId = "openai" | "anthropic" | "ollama";

/** Mirrors `sevak_core::AiConfig`. An API key is never part of it. */
export interface AiConfig {
  enabled: boolean;
  keyword: string;
  provider: AiProviderId;
  model: string;
  base_url: string;
  system_prompt: string;
  max_tokens: number;
  timeout_secs: number;
}

export interface ProviderInfo {
  id: AiProviderId;
  label: string;
  defaultModel: string;
  defaultBaseUrl: string;
  /** The environment variable read when no key is stored; none for Ollama. */
  envVar: string | null;
  /** Where to describe the service in one line. */
  blurb: string;
}

/** The same defaults as `AiProvider` in `sevak_core::ai`. */
export const AI_PROVIDERS: ProviderInfo[] = [
  {
    id: "ollama",
    label: "Ollama (on this computer)",
    defaultModel: "llama3.2",
    defaultBaseUrl: "http://localhost:11434",
    envVar: null,
    blurb: "A model that runs on this computer. Nothing leaves it unless the base URL points elsewhere.",
  },
  {
    id: "openai",
    label: "OpenAI-compatible (ChatGPT, OpenRouter, LM Studio…)",
    defaultModel: "gpt-4o-mini",
    defaultBaseUrl: "https://api.openai.com/v1",
    envVar: "OPENAI_API_KEY",
    blurb: "OpenAI, or any server that speaks the same API: change the base URL.",
  },
  {
    id: "anthropic",
    label: "Anthropic (Claude)",
    defaultModel: "claude-haiku-4-5",
    defaultBaseUrl: "https://api.anthropic.com",
    envVar: "ANTHROPIC_API_KEY",
    blurb: "Anthropic's Messages API.",
  },
];

export const MIN_MAX_TOKENS = 16;
export const MAX_MAX_TOKENS = 8192;
export const MIN_TIMEOUT_SECS = 5;
export const MAX_TIMEOUT_SECS = 300;
export const MAX_SYSTEM_PROMPT_CHARS = 4000;
export const MAX_MODEL_CHARS = 200;
export const DEFAULT_KEYWORD = "ai";

export function providerInfo(id: AiProviderId): ProviderInfo {
  return AI_PROVIDERS.find((p) => p.id === id) ?? AI_PROVIDERS[0];
}

/** What a base URL looks like once taken apart, or why it cannot be used. */
export interface ParsedBase {
  scheme: "http" | "https";
  host: string;
  port: number | null;
}

/** `localhost`, `127.x.x.x` and `::1`: traffic to them never leaves the computer. */
export function isLoopbackHost(host: string): boolean {
  const h = host.trim().replace(/^\[|\]$/g, "").toLowerCase();
  return h === "localhost" || h.endsWith(".localhost") || h === "::1" || /^127(\.\d{1,3}){3}$/.test(h);
}

/**
 * The same rules as `parse_base_url` in Rust: `http://` or `https://`, a host, an
 * optional port and path; no user name, password, query or fragment. Returns
 * the parts, or the text that completes "the base URL …".
 */
export function parseBaseUrl(url: string): ParsedBase | string {
  const text = url.trim();
  const lower = text.toLowerCase();
  let scheme: "http" | "https";
  let rest: string;
  if (lower.startsWith("https://")) {
    scheme = "https";
    rest = lower.slice(8);
  } else if (lower.startsWith("http://")) {
    scheme = "http";
    rest = lower.slice(7);
  } else {
    return "must start with http:// or https://";
  }
  if (/[\s\u0000-\u001f]/.test(text)) return "cannot contain spaces";
  if (/[?#]/.test(rest)) return "cannot contain a query or fragment";
  const authority = rest.split("/")[0];
  if (authority.includes("@")) return "cannot contain a user name or password";
  let host: string;
  let port: string | null = null;
  if (authority.startsWith("[")) {
    const close = authority.indexOf("]");
    if (close < 0) return "has an invalid address";
    host = authority.slice(1, close);
    const after = authority.slice(close + 1);
    if (after.startsWith(":")) port = after.slice(1);
    else if (after !== "") return "has an invalid address";
  } else {
    const colon = authority.lastIndexOf(":");
    host = colon < 0 ? authority : authority.slice(0, colon);
    port = colon < 0 ? null : authority.slice(colon + 1);
  }
  if (host === "") return "needs a host name";
  if (!/^[a-z0-9._:-]+$/.test(host)) return "has an invalid host name";
  let portNumber: number | null = null;
  if (port !== null) {
    if (!/^\d{1,5}$/.test(port) || Number(port) > 65535) return "has an invalid port";
    portNumber = Number(port);
  }
  return { scheme, host, port: portNumber };
}

/** The address a question goes to: the base URL, or the provider's default. */
export function effectiveBaseUrl(ai: AiConfig): string {
  const url = ai.base_url.trim().replace(/\/+$/, "");
  return url === "" ? providerInfo(ai.provider).defaultBaseUrl : url;
}

export function effectiveModel(ai: AiConfig): string {
  return ai.model.trim() === "" ? providerInfo(ai.provider).defaultModel : ai.model.trim();
}

/** `host` or `host:port` of where questions go; empty when the base URL is unusable. */
export function destinationHost(ai: AiConfig): string {
  const parsed = parseBaseUrl(effectiveBaseUrl(ai));
  if (typeof parsed === "string") return "";
  const host = parsed.host.includes(":") ? `[${parsed.host}]` : parsed.host;
  return parsed.port === null ? host : `${host}:${parsed.port}`;
}

/** Whether questions stay on this computer. */
export function staysOnThisComputer(ai: AiConfig): boolean {
  const parsed = parseBaseUrl(effectiveBaseUrl(ai));
  return typeof parsed !== "string" && isLoopbackHost(parsed.host);
}

/**
 * The plain statement of what is sent and where, shown above the form. It is
 * built from the live settings, so it is true for what the user is editing.
 */
export function disclosure(ai: AiConfig): string {
  const host = destinationHost(ai);
  const where = host === "" ? "the address in Base URL" : host;
  const keyword = ai.keyword.trim() || DEFAULT_KEYWORD;
  const sent = `the question you typed after “${keyword} ” (or the text you chose with “Ask AI about selection”), the system prompt below and the model name`;
  if (staysOnThisComputer(ai)) {
    return `When you press Enter on a question, Sevak sends ${sent} to ${where}, which is this computer. Nothing leaves it.`;
  }
  return `When you press Enter on a question, Sevak sends ${sent} to ${where}, over the internet. Nothing else is sent: not your clipboard, files or search history.`;
}

/** The keyword is checked with the other keywords (see validate.ts). */
export type AiField = "model" | "baseUrl" | "systemPrompt" | "maxTokens" | "timeout";
export type AiProblems = Partial<Record<AiField, string>>;

/** The checks `AiConfig::problem` makes in Rust, per field. */
export function aiProblems(ai: AiConfig): AiProblems {
  const problems: AiProblems = {};
  if ([...ai.model].length > MAX_MODEL_CHARS) problems.model = "Too long";
  if ([...ai.system_prompt].length > MAX_SYSTEM_PROMPT_CHARS) {
    problems.systemPrompt = `At most ${MAX_SYSTEM_PROMPT_CHARS} characters`;
  }
  if (ai.base_url.trim() !== "") {
    const parsed = parseBaseUrl(ai.base_url);
    if (typeof parsed === "string") problems.baseUrl = `The base URL ${parsed}`;
  }
  if (!Number.isInteger(ai.max_tokens) || ai.max_tokens < MIN_MAX_TOKENS || ai.max_tokens > MAX_MAX_TOKENS) {
    problems.maxTokens = `Enter a whole number from ${MIN_MAX_TOKENS} to ${MAX_MAX_TOKENS}`;
  }
  if (!Number.isInteger(ai.timeout_secs) || ai.timeout_secs < MIN_TIMEOUT_SECS || ai.timeout_secs > MAX_TIMEOUT_SECS) {
    problems.timeout = `Enter a whole number from ${MIN_TIMEOUT_SECS} to ${MAX_TIMEOUT_SECS}`;
  }
  return problems;
}

/**
 * The config after switching provider. A model or base URL that was only the old
 * provider's default (or empty) follows the switch; one the user typed stays.
 */
export function switchProvider(ai: AiConfig, next: AiProviderId): AiConfig {
  const old = providerInfo(ai.provider);
  const model = ai.model.trim();
  const base = ai.base_url.trim().replace(/\/+$/, "");
  return {
    ...ai,
    provider: next,
    model: model === "" || model === old.defaultModel ? "" : ai.model,
    base_url: base === "" || base === old.defaultBaseUrl ? "" : ai.base_url,
  };
}

/** What the key field's owner state means, mirroring `KeyInfo` in the shell. */
export interface KeyInfo {
  state: "none" | "stored" | "env" | "unreadable";
  env_var: string | null;
}

export interface KeyStatus {
  openai: KeyInfo;
  anthropic: KeyInfo;
  /** How a stored key is kept, in words. */
  protection: string;
}

/** One line about the key, never the key. `null` for a provider without one. */
export function keyStatusText(provider: AiProviderId, status: KeyStatus | null): string | null {
  if (provider === "ollama") return null;
  if (status === null) return "";
  const info = status[provider];
  switch (info.state) {
    case "stored":
      return `A key is saved, ${status.protection}.`;
    case "env":
      return `Using the ${info.env_var ?? "environment"} environment variable. A key saved here would take its place.`;
    case "unreadable":
      return "A saved key cannot be read (it was saved by another user or computer). Enter it again.";
    default:
      return info.env_var
        ? `No key yet. Paste one here, or set the ${info.env_var} environment variable.`
        : "No key yet.";
  }
}

/** Whether the provider needs a key before it can answer (a local server may not). */
export function needsKey(ai: AiConfig): boolean {
  if (ai.provider === "ollama") return false;
  if (ai.provider === "anthropic") return true;
  return !staysOnThisComputer(ai);
}

/** Whether an API key typed into the field is plausible (as `ApiKey::new` in Rust). */
export function isPlausibleKey(text: string): boolean {
  const key = text.trim();
  return key.length > 0 && key.length <= 512 && /^[\x21-\x7e]+$/.test(key);
}
