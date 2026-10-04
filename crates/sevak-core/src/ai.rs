//! The `[ai]` settings: which assistant `ai <question>` talks to.
//!
//! Plain data and validation only; the HTTP clients live in
//! `sevak_plugins::ai`. Nothing here holds an API key: keys are never part of
//! the configuration file (see `sevak_plugins::ai::keys`).

use serde::{Deserialize, Serialize};

/// The default keyword (`ai <question>`).
pub const DEFAULT_KEYWORD: &str = "ai";
/// The system prompt used until the user writes their own.
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are a concise assistant inside a desktop launcher. \
Answer briefly in plain text, without Markdown headings.";

/// Smallest and largest answer length (in tokens) that can be requested.
pub const MIN_MAX_TOKENS: u32 = 16;
pub const MAX_MAX_TOKENS: u32 = 8_192;
/// Shortest and longest wait for an answer, in seconds.
pub const MIN_TIMEOUT_SECS: u32 = 5;
pub const MAX_TIMEOUT_SECS: u32 = 300;
/// Longest system prompt, model name and base URL kept (in characters).
pub const MAX_SYSTEM_PROMPT_CHARS: usize = 4_000;
pub const MAX_MODEL_CHARS: usize = 200;
pub const MAX_BASE_URL_CHARS: usize = 2_048;

/// The service that answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    /// OpenAI's chat completions API, or any server that speaks it (OpenRouter,
    /// Groq, LM Studio, vLLM, llama.cpp, ...).
    #[serde(rename = "openai")]
    OpenAi,
    /// Anthropic's Messages API.
    #[serde(rename = "anthropic")]
    Anthropic,
    /// Ollama on this computer (or the address in `base_url`). The default, and
    /// what any unrecognised value in the file means: nothing leaves the
    /// computer unless `base_url` points elsewhere.
    #[default]
    #[serde(other)]
    Ollama,
}

impl AiProvider {
    pub const ALL: [Self; 3] = [Self::OpenAi, Self::Anthropic, Self::Ollama];

    /// The value written in `config.toml` (`provider = "..."`).
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Ollama => "ollama",
        }
    }

    /// How the provider is named in the interface.
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI-compatible",
            Self::Anthropic => "Anthropic",
            Self::Ollama => "Ollama",
        }
    }

    /// Where requests go when `base_url` is empty.
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Ollama => "http://localhost:11434",
        }
    }

    /// The model used when `model` is empty. Only a starting point: providers
    /// retire models, so the Settings page lets you type any name.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-4o-mini",
            Self::Anthropic => "claude-haiku-4-5",
            Self::Ollama => "llama3.2",
        }
    }

    /// The environment variable that supplies an API key when none is stored.
    pub fn key_env_var(self) -> Option<&'static str> {
        match self {
            Self::OpenAi => Some("OPENAI_API_KEY"),
            Self::Anthropic => Some("ANTHROPIC_API_KEY"),
            Self::Ollama => None,
        }
    }

    /// Parses the value of `provider = "..."` (case-insensitive); `None` for
    /// anything else.
    pub fn from_id(id: &str) -> Option<Self> {
        let id = id.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|p| p.id() == id)
    }
}

/// The AI assistant (`ai <question>`). Opt-in: nothing is sent anywhere unless
/// `enabled` is set, and then only when the user presses Enter on a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    pub enabled: bool,
    pub keyword: String,
    pub provider: AiProvider,
    /// Empty uses the provider's default model.
    pub model: String,
    /// Empty uses the provider's default address.
    pub base_url: String,
    pub system_prompt: String,
    /// Longest answer asked for, in tokens.
    pub max_tokens: u32,
    /// How long to wait for the whole answer, in seconds.
    pub timeout_secs: u32,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            keyword: DEFAULT_KEYWORD.to_owned(),
            provider: AiProvider::default(),
            model: String::new(),
            base_url: String::new(),
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_owned(),
            max_tokens: 512,
            timeout_secs: 60,
        }
    }
}

impl AiConfig {
    /// The model that will be asked: `model`, or the provider's default.
    pub fn effective_model(&self) -> &str {
        let model = self.model.trim();
        if model.is_empty() {
            self.provider.default_model()
        } else {
            model
        }
    }

    /// The address requests go to: `base_url` without a trailing slash, or the
    /// provider's default.
    pub fn effective_base_url(&self) -> String {
        let url = self.base_url.trim().trim_end_matches('/');
        if url.is_empty() {
            self.provider.default_base_url().to_owned()
        } else {
            url.to_owned()
        }
    }

    /// Clamps values into their supported ranges (called by `Config::normalized`).
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.keyword = self.keyword.trim().to_owned();
        if self.keyword.is_empty() || self.keyword.contains(char::is_whitespace) {
            self.keyword = DEFAULT_KEYWORD.to_owned();
        }
        self.model = truncate_chars(self.model.trim(), MAX_MODEL_CHARS);
        self.base_url = truncate_chars(
            self.base_url.trim().trim_end_matches('/'),
            MAX_BASE_URL_CHARS,
        );
        self.system_prompt = truncate_chars(self.system_prompt.trim(), MAX_SYSTEM_PROMPT_CHARS);
        self.max_tokens = self.max_tokens.clamp(MIN_MAX_TOKENS, MAX_MAX_TOKENS);
        self.timeout_secs = self.timeout_secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS);
        self
    }

    /// What is wrong with these settings, in one sentence, or `None`. The
    /// settings window and the shell check the same rules; a value
    /// `normalized` would silently clamp is reported instead.
    pub fn problem(&self) -> Option<String> {
        if !(MIN_MAX_TOKENS..=MAX_MAX_TOKENS).contains(&self.max_tokens) {
            return Some(format!(
                "AI assistant: the answer length must be from {MIN_MAX_TOKENS} to {MAX_MAX_TOKENS} tokens."
            ));
        }
        if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&self.timeout_secs) {
            return Some(format!(
                "AI assistant: the timeout must be from {MIN_TIMEOUT_SECS} to {MAX_TIMEOUT_SECS} seconds."
            ));
        }
        if self.model.chars().count() > MAX_MODEL_CHARS {
            return Some("AI assistant: the model name is too long.".to_owned());
        }
        if self.system_prompt.chars().count() > MAX_SYSTEM_PROMPT_CHARS {
            return Some(format!(
                "AI assistant: the system prompt can have at most {MAX_SYSTEM_PROMPT_CHARS} characters."
            ));
        }
        if !self.base_url.trim().is_empty() {
            if let Err(reason) = parse_base_url(&self.base_url) {
                return Some(format!("AI assistant: the base URL {reason}."));
            }
        }
        None
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A base URL taken apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    /// `http` or `https`, lowercase.
    pub scheme: String,
    /// Host name or address, lowercase, without brackets or port.
    pub host: String,
    pub port: Option<u16>,
}

impl BaseUrl {
    /// `host` or `host:port`, for showing where data goes.
    pub fn authority(&self) -> String {
        match self.port {
            Some(port) if self.host.contains(':') => format!("[{}]:{port}", self.host),
            Some(port) => format!("{}:{port}", self.host),
            None if self.host.contains(':') => format!("[{}]", self.host),
            None => self.host.clone(),
        }
    }

    /// Whether the address is this computer (`localhost`, `127.x.x.x`, `::1`):
    /// traffic to it never leaves the machine, so plain `http` is fine there.
    pub fn is_loopback(&self) -> bool {
        is_loopback_host(&self.host)
    }
}

/// Whether `host` names this computer.
pub fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "::1"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Parses a base URL for the assistant: `http://` or `https://`, a host, an
/// optional port and path; no user name or password, query or fragment.
/// The error completes the sentence "the base URL ...".
pub fn parse_base_url(url: &str) -> Result<BaseUrl, &'static str> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let (scheme, rest) = if let Some(rest) = lower.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = lower.strip_prefix("http://") {
        ("http", rest)
    } else {
        return Err("must start with http:// or https://");
    };
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("cannot contain spaces");
    }
    if rest.contains(['?', '#']) {
        return Err("cannot contain a query or fragment");
    }
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.contains('@') {
        return Err("cannot contain a user name or password");
    }
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let (host, after) = inner.split_once(']').ok_or("has an invalid address")?;
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port),
            None if after.is_empty() => None,
            None => return Err("has an invalid address"),
        };
        (host, port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return Err("needs a host name");
    }
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
    {
        return Err("has an invalid host name");
    }
    let port = match port {
        None => None,
        Some(port) => Some(port.parse::<u16>().map_err(|_| "has an invalid port")?),
    };
    Ok(BaseUrl {
        scheme: scheme.to_owned(),
        host: host.to_owned(),
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_off_and_local() {
        let config = AiConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.keyword, "ai");
        assert_eq!(config.provider, AiProvider::Ollama);
        assert_eq!(config.effective_base_url(), "http://localhost:11434");
        assert_eq!(config.effective_model(), "llama3.2");
        assert_eq!(config.problem(), None);
    }

    #[test]
    fn providers_parse_from_the_file_and_an_unknown_one_is_the_local_one() {
        let parse = |text: &str| -> AiConfig { toml::from_str(text).unwrap() };
        assert_eq!(parse("provider = \"openai\"").provider, AiProvider::OpenAi);
        assert_eq!(
            parse("provider = \"anthropic\"").provider,
            AiProvider::Anthropic
        );
        assert_eq!(parse("provider = \"ollama\"").provider, AiProvider::Ollama);
        // A typo must never turn into a request to a cloud service.
        assert_eq!(parse("provider = \"gemini\"").provider, AiProvider::Ollama);
        assert_eq!(parse("").provider, AiProvider::Ollama);
    }

    #[test]
    fn a_partial_section_keeps_the_other_defaults() {
        let config: AiConfig = toml::from_str("enabled = true\nmax_tokens = 100").unwrap();
        assert!(config.enabled);
        assert_eq!(config.max_tokens, 100);
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.keyword, "ai");
    }

    #[test]
    fn provider_ids_round_trip() {
        for provider in AiProvider::ALL {
            assert_eq!(AiProvider::from_id(provider.id()), Some(provider));
        }
        assert_eq!(AiProvider::from_id(" OpenAI "), Some(AiProvider::OpenAi));
        assert_eq!(AiProvider::from_id("gemini"), None);
    }

    #[test]
    fn normalizing_clamps_and_trims() {
        let config = AiConfig {
            keyword: "  two words ".to_owned(),
            model: "  gpt-4o  ".to_owned(),
            base_url: " http://localhost:1234/v1/// ".to_owned(),
            max_tokens: 1_000_000,
            timeout_secs: 0,
            system_prompt: "  be brief ".to_owned(),
            ..AiConfig::default()
        }
        .normalized();
        assert_eq!(config.keyword, "ai");
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.base_url, "http://localhost:1234/v1");
        assert_eq!(config.max_tokens, MAX_MAX_TOKENS);
        assert_eq!(config.timeout_secs, MIN_TIMEOUT_SECS);
        assert_eq!(config.system_prompt, "be brief");
        assert_eq!(
            AiConfig {
                keyword: "ask".to_owned(),
                ..AiConfig::default()
            }
            .normalized()
            .keyword,
            "ask"
        );
    }

    #[test]
    fn problems_are_reported_not_clamped() {
        let ok = AiConfig::default();
        assert_eq!(ok.problem(), None);
        let bad = |change: fn(&mut AiConfig)| {
            let mut config = AiConfig::default();
            change(&mut config);
            config.problem().expect("a problem")
        };
        assert!(bad(|c| c.max_tokens = 0).contains("tokens"));
        assert!(bad(|c| c.timeout_secs = 1).contains("timeout"));
        assert!(bad(|c| c.timeout_secs = 10_000).contains("timeout"));
        assert!(bad(|c| c.base_url = "ftp://x".into()).contains("http"));
        assert!(bad(|c| c.system_prompt = "x".repeat(5_000)).contains("system prompt"));
    }

    #[test]
    fn base_urls_are_taken_apart() {
        let url = parse_base_url("https://API.openai.com/v1").unwrap();
        assert_eq!(url.scheme, "https");
        assert_eq!(url.host, "api.openai.com");
        assert_eq!(url.port, None);
        assert!(!url.is_loopback());
        assert_eq!(url.authority(), "api.openai.com");

        let local = parse_base_url("http://localhost:11434").unwrap();
        assert_eq!(local.port, Some(11434));
        assert!(local.is_loopback());
        assert_eq!(local.authority(), "localhost:11434");

        let v6 = parse_base_url("http://[::1]:8080/v1").unwrap();
        assert_eq!(v6.host, "::1");
        assert!(v6.is_loopback());
        assert_eq!(v6.authority(), "[::1]:8080");

        assert!(parse_base_url("http://127.0.0.1:1234")
            .unwrap()
            .is_loopback());
        assert!(!parse_base_url("http://192.168.1.5:11434")
            .unwrap()
            .is_loopback());
        assert!(!parse_base_url("https://localhost.evil.com")
            .unwrap()
            .is_loopback());
    }

    #[test]
    fn unsafe_or_broken_base_urls_are_refused() {
        for bad in [
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
        ] {
            assert!(parse_base_url(bad).is_err(), "{bad:?} should be refused");
        }
    }
}
