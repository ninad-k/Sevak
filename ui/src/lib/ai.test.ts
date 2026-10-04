import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AI_PROVIDERS,
  aiProblems,
  destinationHost,
  disclosure,
  effectiveBaseUrl,
  effectiveModel,
  isLoopbackHost,
  isPlausibleKey,
  keyStatusText,
  needsKey,
  parseBaseUrl,
  providerInfo,
  staysOnThisComputer,
  switchProvider,
  type AiConfig,
  type KeyStatus,
} from "./ai";
import { mockSettings } from "./mock";
import type { Config } from "./settings-ipc";
import { tidyPluginSettings } from "./settings-pages/tidy";
import { totalProblems, validate } from "./validate";

beforeEach(() => {
  vi.stubGlobal("location", { search: "" });
});

const ai = (extra: Partial<AiConfig> = {}): AiConfig => ({
  enabled: true,
  keyword: "ai",
  provider: "ollama",
  model: "",
  base_url: "",
  system_prompt: "Be brief.",
  max_tokens: 512,
  timeout_secs: 60,
  ...extra,
});

const config = (extra: Partial<AiConfig> = {}): Config => {
  const c = structuredClone(mockSettings().config);
  c.ai = ai(extra);
  return c;
};

describe("providers", () => {
  it("have the defaults Rust uses", () => {
    expect(providerInfo("ollama").defaultBaseUrl).toBe("http://localhost:11434");
    expect(providerInfo("openai").defaultBaseUrl).toBe("https://api.openai.com/v1");
    expect(providerInfo("anthropic").defaultBaseUrl).toBe("https://api.anthropic.com");
    expect(providerInfo("openai").envVar).toBe("OPENAI_API_KEY");
    expect(providerInfo("anthropic").envVar).toBe("ANTHROPIC_API_KEY");
    expect(providerInfo("ollama").envVar).toBeNull();
    expect(new Set(AI_PROVIDERS.map((p) => p.id)).size).toBe(3);
  });

  it("fill in the model and the address when the fields are empty", () => {
    expect(effectiveModel(ai({ provider: "openai" }))).toBe("gpt-4o-mini");
    expect(effectiveModel(ai({ provider: "openai", model: " gpt-4o " }))).toBe("gpt-4o");
    expect(effectiveBaseUrl(ai({ provider: "anthropic" }))).toBe("https://api.anthropic.com");
    expect(effectiveBaseUrl(ai({ base_url: "http://localhost:1234/v1///" }))).toBe(
      "http://localhost:1234/v1",
    );
  });
});

describe("switchProvider", () => {
  it("lets a model and address that were only the old defaults follow the switch", () => {
    const next = switchProvider(
      ai({ provider: "openai", model: "gpt-4o-mini", base_url: "https://api.openai.com/v1/" }),
      "anthropic",
    );
    expect(next.provider).toBe("anthropic");
    expect(next.model).toBe("");
    expect(next.base_url).toBe("");
  });

  it("keeps what the user typed", () => {
    const next = switchProvider(
      ai({ provider: "openai", model: "my-model", base_url: "http://localhost:1234/v1" }),
      "ollama",
    );
    expect(next.model).toBe("my-model");
    expect(next.base_url).toBe("http://localhost:1234/v1");
    expect(next.system_prompt).toBe("Be brief.");
    expect(next.enabled).toBe(true);
  });

  it("does not change the object it was given", () => {
    const before = ai({ provider: "openai", model: "gpt-4o-mini" });
    switchProvider(before, "ollama");
    expect(before.provider).toBe("openai");
    expect(before.model).toBe("gpt-4o-mini");
  });
});

describe("parseBaseUrl (the same rules as Rust)", () => {
  it("takes an address apart", () => {
    expect(parseBaseUrl("https://API.openai.com/v1")).toEqual({
      scheme: "https",
      host: "api.openai.com",
      port: null,
    });
    expect(parseBaseUrl("http://localhost:11434")).toEqual({
      scheme: "http",
      host: "localhost",
      port: 11434,
    });
    expect(parseBaseUrl("http://[::1]:8080/v1")).toEqual({ scheme: "http", host: "::1", port: 8080 });
  });

  it("refuses what cannot be used or could leak a credential", () => {
    for (const bad of [
      "",
      "localhost:11434",
      "ftp://example.com",
      "file:///etc/passwd",
      "https://user:pass@example.com",
      "https://example.com/v1?key=1",
      "https://example.com/#frag",
      "https://",
      "https://:80",
      "https://exa mple.com",
      "https://example.com:notaport",
      "https://example.com:99999",
      "http://[::1",
      "https://exa$mple.com",
    ]) {
      expect(typeof parseBaseUrl(bad), bad).toBe("string");
    }
  });

  it("knows which hosts are this computer", () => {
    for (const host of ["localhost", "LOCALHOST", "127.0.0.1", "127.1.2.3", "::1", "[::1]", "a.localhost"]) {
      expect(isLoopbackHost(host), host).toBe(true);
    }
    for (const host of ["localhost.evil.com", "192.168.1.5", "api.openai.com", "128.0.0.1"]) {
      expect(isLoopbackHost(host), host).toBe(false);
    }
  });
});

describe("disclosure", () => {
  it("says a local model keeps everything on this computer", () => {
    const text = disclosure(ai());
    expect(text).toContain("localhost:11434");
    expect(text).toContain("this computer");
    expect(text).toContain("Nothing leaves it");
    expect(staysOnThisComputer(ai())).toBe(true);
  });

  it("names the host a cloud question goes to, and what else is not sent", () => {
    const text = disclosure(ai({ provider: "openai" }));
    expect(text).toContain("api.openai.com");
    expect(text).toContain("over the internet");
    expect(text).toContain("not your clipboard, files or search history");
    expect(text).toContain("system prompt");
    expect(destinationHost(ai({ provider: "anthropic" }))).toBe("api.anthropic.com");
  });

  it("follows the base URL being edited, and the keyword", () => {
    expect(destinationHost(ai({ provider: "openai", base_url: "http://localhost:1234/v1" }))).toBe(
      "localhost:1234",
    );
    expect(staysOnThisComputer(ai({ provider: "openai", base_url: "http://localhost:1234/v1" }))).toBe(true);
    expect(staysOnThisComputer(ai({ provider: "openai", base_url: "http://192.168.1.5:8000/v1" }))).toBe(false);
    expect(disclosure(ai({ provider: "openai", base_url: "nonsense" }))).toContain("the address in Base URL");
    expect(disclosure(ai({ keyword: "ask" }))).toContain("“ask ”");
  });
});

describe("aiProblems", () => {
  it("accepts the defaults", () => {
    expect(aiProblems(ai())).toEqual({});
  });

  it("flags each field Rust would refuse", () => {
    expect(aiProblems(ai({ max_tokens: 0 })).maxTokens).toBeDefined();
    expect(aiProblems(ai({ max_tokens: 100000 })).maxTokens).toBeDefined();
    expect(aiProblems(ai({ max_tokens: 1.5 })).maxTokens).toBeDefined();
    expect(aiProblems(ai({ max_tokens: Number.NaN })).maxTokens).toBeDefined();
    expect(aiProblems(ai({ timeout_secs: 1 })).timeout).toBeDefined();
    expect(aiProblems(ai({ timeout_secs: 301 })).timeout).toBeDefined();
    expect(aiProblems(ai({ base_url: "ftp://x" })).baseUrl).toContain("http");
    expect(aiProblems(ai({ system_prompt: "x".repeat(4001) })).systemPrompt).toBeDefined();
    expect(aiProblems(ai({ model: "m".repeat(201) })).model).toBeDefined();
    expect(aiProblems(ai({ base_url: "  " }))).toEqual({});
  });
});

describe("validate integration", () => {
  it("counts AI problems against the AI page", () => {
    const bad = config({ max_tokens: 0, base_url: "ftp://x" });
    const problems = validate(bad);
    expect(problems.count.ai).toBe(2);
    expect(totalProblems(problems)).toBeGreaterThanOrEqual(2);
    expect(totalProblems(validate(config()))).toBe(0);
  });

  it("treats the keyword like the other built-in keywords", () => {
    const empty = validate(config({ keyword: "" }));
    expect(empty.keywords["ai.keyword"]).toBe("Required");
    expect(empty.count.ai).toBe(1);
    expect(validate(config({ keyword: "two words" })).keywords["ai.keyword"]).toBe("No spaces");

    const clash = config({ keyword: "g" });
    expect(validate(clash).keywords["ai.keyword"]).toBe("Already a web search keyword");

    const withTasks = config({ keyword: "cb" });
    expect(validate(withTasks).keywords["ai.keyword"]).toBe("Already used by clipboard history");

    const other = config();
    other.contacts.keyword = "ai";
    const problems = validate(other);
    expect(problems.keywords["ai.keyword"]).toBe("Already used by contacts");
    expect(problems.keywords["contacts.keyword"]).toBe("Already used by the AI assistant");
  });
});

describe("tidyPluginSettings", () => {
  it("trims the AI fields as Rust's normalization does", () => {
    const c = config({ keyword: " ai ", model: " m ", base_url: " http://x.test/v1/// ", system_prompt: "  hi " });
    tidyPluginSettings(c);
    expect(c.ai.keyword).toBe("ai");
    expect(c.ai.model).toBe("m");
    expect(c.ai.base_url).toBe("http://x.test/v1");
    expect(c.ai.system_prompt).toBe("hi");
  });
});

describe("the API key", () => {
  const status = (over: Partial<KeyStatus> = {}): KeyStatus => ({
    openai: { state: "none", env_var: "OPENAI_API_KEY" },
    anthropic: { state: "none", env_var: "ANTHROPIC_API_KEY" },
    protection: "encrypted with Windows DPAPI for your user account",
    ...over,
  });

  it("is described, never shown", () => {
    expect(keyStatusText("ollama", status())).toBeNull();
    expect(keyStatusText("openai", null)).toBe("");
    expect(keyStatusText("openai", status())).toContain("OPENAI_API_KEY");
    expect(
      keyStatusText("openai", status({ openai: { state: "stored", env_var: "OPENAI_API_KEY" } })),
    ).toContain("encrypted with Windows DPAPI");
    expect(
      keyStatusText("anthropic", status({ anthropic: { state: "env", env_var: "ANTHROPIC_API_KEY" } })),
    ).toContain("ANTHROPIC_API_KEY environment variable");
    expect(
      keyStatusText("openai", status({ openai: { state: "unreadable", env_var: "OPENAI_API_KEY" } })),
    ).toContain("Enter it again");
  });

  it("is required only where it must be", () => {
    expect(needsKey(ai({ provider: "ollama" }))).toBe(false);
    expect(needsKey(ai({ provider: "anthropic" }))).toBe(true);
    expect(needsKey(ai({ provider: "openai" }))).toBe(true);
    expect(needsKey(ai({ provider: "openai", base_url: "http://localhost:1234/v1" }))).toBe(false);
  });

  it("accepts only something that could be a key", () => {
    expect(isPlausibleKey("sk-abc123_DEF")).toBe(true);
    expect(isPlausibleKey("  sk-abc  ")).toBe(true);
    for (const bad of ["", "   ", "two words", "line\nbreak", "café", "k".repeat(513)]) {
      expect(isPlausibleKey(bad), bad).toBe(false);
    }
  });

  it("is not a field of the configuration", () => {
    expect(Object.keys(config().ai).some((name) => /^(api_?)?key$|secret|password/i.test(name))).toBe(false);
    expect(JSON.stringify(config())).not.toMatch(/api_?key/i);
  });
});
