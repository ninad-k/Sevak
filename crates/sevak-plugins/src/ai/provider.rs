//! The three services the assistant can ask, behind one trait.
//!
//! A [`Provider`] only builds requests and reads replies; it never touches the
//! network ([`super::http`] does) and never sees more than the one API key it is
//! handed for the request. Every reply is untrusted text: [`clean_answer`]
//! strips control and direction-override characters and caps the length before
//! anything is shown.
//!
//! | Provider | Chat | Connection test (no prompt is sent) |
//! |---|---|---|
//! | OpenAI-compatible | `POST {base}/chat/completions`, `Authorization: Bearer` | `GET {base}/models` |
//! | Anthropic | `POST {base}/v1/messages`, `x-api-key`, `anthropic-version` | `GET {base}/v1/models` |
//! | Ollama | `POST {base}/api/chat` with `stream: false` | `GET {base}/api/tags` (also checks the model is installed) |

use std::time::Duration;

use serde_json::{json, Value};
use sevak_core::ai::{parse_base_url, BaseUrl};
use sevak_core::{AiConfig, AiProvider};

use super::error::{AiError, MAX_SERVER_MESSAGE_CHARS};
use super::http::{HttpRequest, HttpResponse, Method};
use super::keys::{scrub, ApiKey};

/// The most of a reply that is read; anything longer is an error.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// The most of an answer that is kept (characters).
pub const MAX_ANSWER_CHARS: usize = 20_000;
/// The API version header Anthropic requires.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The configuration of one request, resolved from [`AiConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub provider: AiProvider,
    /// No trailing slash.
    pub base_url: String,
    pub model: String,
    pub system_prompt: String,
    pub max_tokens: u32,
    pub timeout: Duration,
}

impl Settings {
    pub fn from_config(config: &AiConfig) -> Self {
        Self {
            provider: config.provider,
            base_url: config.effective_base_url(),
            model: config.effective_model().to_owned(),
            system_prompt: config.system_prompt.trim().to_owned(),
            max_tokens: config.max_tokens,
            timeout: Duration::from_secs(u64::from(config.timeout_secs)),
        }
    }
}

/// What a model said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// Plain text, cleaned and capped.
    pub text: String,
    /// The model stopped at its length limit, or the text was cut to the cap.
    pub cut_off: bool,
}

/// One AI service's wire format.
pub trait Provider: Send + Sync {
    fn kind(&self) -> AiProvider;

    /// Whether a request to `base` must carry an API key. A local server
    /// (LM Studio, Ollama's OpenAI-compatible endpoint) usually has none.
    fn needs_key(&self, base: &BaseUrl) -> bool;

    /// The request that asks `prompt`.
    fn chat_request(&self, settings: &Settings, key: Option<&ApiKey>, prompt: &str) -> HttpRequest;

    /// Reads the reply to [`Provider::chat_request`].
    fn parse_chat(&self, settings: &Settings, response: &HttpResponse) -> Result<Answer, AiError>;

    /// A request that checks the address and the key without sending a prompt.
    fn probe_request(&self, settings: &Settings, key: Option<&ApiKey>) -> HttpRequest;

    /// Reads the reply to [`Provider::probe_request`] into a sentence for the
    /// person who pressed "Test connection".
    fn parse_probe(&self, settings: &Settings, response: &HttpResponse) -> Result<String, AiError>;
}

/// The implementation for `kind`.
pub fn provider_for(kind: AiProvider) -> Box<dyn Provider> {
    match kind {
        AiProvider::OpenAi => Box::new(OpenAiCompatible),
        AiProvider::Anthropic => Box::new(Anthropic),
        AiProvider::Ollama => Box::new(Ollama),
    }
}

fn request(
    settings: &Settings,
    method: Method,
    url: String,
    mut headers: Vec<(String, String)>,
    body: Option<Value>,
) -> HttpRequest {
    headers.push(("Accept".to_owned(), "application/json".to_owned()));
    let body = body.map(|value| {
        headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
        serde_json::to_vec(&value).unwrap_or_default()
    });
    HttpRequest {
        method,
        url,
        headers,
        body,
        timeout: settings.timeout,
        max_response_bytes: MAX_RESPONSE_BYTES,
    }
}

fn messages(settings: &Settings, prompt: &str) -> Vec<Value> {
    let mut messages = Vec::with_capacity(2);
    if !settings.system_prompt.is_empty() {
        messages.push(json!({"role": "system", "content": settings.system_prompt}));
    }
    messages.push(json!({"role": "user", "content": prompt}));
    messages
}

/// The error message a server wrote, cleaned and short; empty if it wrote none.
fn server_message(body: &[u8]) -> String {
    let Ok(json) = serde_json::from_slice::<Value>(body) else {
        return String::new();
    };
    let message = json
        .pointer("/error/message")
        .or_else(|| json.get("error"))
        .or_else(|| json.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let one_line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    scrub(&one_line, None)
        .chars()
        .take(MAX_SERVER_MESSAGE_CHARS)
        .collect()
}

/// The error for a reply whose status is not 2xx.
fn http_error(settings: &Settings, response: &HttpResponse) -> AiError {
    let status = response.status;
    let message = server_message(&response.body);
    match status {
        401 | 403 => AiError::Unauthorized {
            provider: settings.provider,
        },
        404 if message.to_lowercase().contains("model") => AiError::ModelNotFound {
            model: settings.model.clone(),
            provider: settings.provider,
        },
        429 => AiError::RateLimited,
        500..=599 => AiError::Server { status },
        // A redirect is never followed (it could carry the key elsewhere).
        300..=399 => AiError::Rejected {
            status,
            message: "the address redirects somewhere else; use the final address as the base URL"
                .to_owned(),
        },
        _ => AiError::Rejected { status, message },
    }
}

fn is_success(response: &HttpResponse) -> bool {
    (200..300).contains(&response.status)
}

fn json_body(response: &HttpResponse) -> Result<Value, AiError> {
    serde_json::from_slice(&response.body).map_err(|_| AiError::Malformed)
}

/// Turns what a model said into text that is safe to show and paste: carriage
/// returns, control characters and the characters that reorder text direction
/// (used to disguise what text says) are removed, and the length is capped.
/// The second value is true when the cap cut something off.
pub fn clean_answer(text: &str) -> (String, bool) {
    let cleaned: String = text
        .chars()
        .filter(|&c| match c {
            '\n' | '\t' => true,
            c if c.is_control() => false,
            // Explicit bidirectional embeddings, overrides and isolates.
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => false,
            _ => true,
        })
        .collect();
    let cleaned = cleaned.trim();
    let mut chars = cleaned.chars();
    let kept: String = chars.by_ref().take(MAX_ANSWER_CHARS).collect();
    let clipped = chars.next().is_some();
    (kept, clipped)
}

fn answer_from(text: &str, model_cut_off: bool) -> Result<Answer, AiError> {
    let (text, clipped) = clean_answer(text);
    if text.is_empty() {
        return Err(AiError::Empty {
            cut_off: model_cut_off,
        });
    }
    Ok(Answer {
        text,
        cut_off: model_cut_off || clipped,
    })
}

/// "5 models" / "1 model".
fn count_models(count: usize) -> String {
    if count == 1 {
        "1 model".to_owned()
    } else {
        format!("{count} models")
    }
}

fn host_of(settings: &Settings) -> String {
    parse_base_url(&settings.base_url)
        .map_or_else(|_| settings.base_url.clone(), |url| url.authority())
}

// ---------------------------------------------------------------------------

/// OpenAI's chat completions API, and the many servers that copy it.
pub struct OpenAiCompatible;

impl OpenAiCompatible {
    fn headers(key: Option<&ApiKey>) -> Vec<(String, String)> {
        key.map(|key| {
            (
                "Authorization".to_owned(),
                format!("Bearer {}", key.expose()),
            )
        })
        .into_iter()
        .collect()
    }
}

impl Provider for OpenAiCompatible {
    fn kind(&self) -> AiProvider {
        AiProvider::OpenAi
    }

    fn needs_key(&self, base: &BaseUrl) -> bool {
        !base.is_loopback()
    }

    fn chat_request(&self, settings: &Settings, key: Option<&ApiKey>, prompt: &str) -> HttpRequest {
        // OpenAI's own reasoning models reject `max_tokens`; other servers
        // mostly still only know it.
        let official =
            parse_base_url(&settings.base_url).is_ok_and(|url| url.host == "api.openai.com");
        let limit = if official {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        let mut body = json!({
            "model": settings.model,
            "messages": messages(settings, prompt),
        });
        body[limit] = json!(settings.max_tokens);
        request(
            settings,
            Method::Post,
            format!("{}/chat/completions", settings.base_url),
            Self::headers(key),
            Some(body),
        )
    }

    fn parse_chat(&self, settings: &Settings, response: &HttpResponse) -> Result<Answer, AiError> {
        if !is_success(response) {
            return Err(http_error(settings, response));
        }
        let json = json_body(response)?;
        let choice = json.pointer("/choices/0").ok_or(AiError::Malformed)?;
        let cut_off = choice.get("finish_reason").and_then(Value::as_str) == Some("length");
        let content = choice.pointer("/message/content");
        let text = match content {
            Some(Value::String(text)) => text.clone(),
            // Some servers send a list of typed parts.
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(""),
            Some(Value::Null) | None => String::new(),
            Some(_) => return Err(AiError::Malformed),
        };
        answer_from(&text, cut_off)
    }

    fn probe_request(&self, settings: &Settings, key: Option<&ApiKey>) -> HttpRequest {
        request(
            settings,
            Method::Get,
            format!("{}/models", settings.base_url),
            Self::headers(key),
            None,
        )
    }

    fn parse_probe(&self, settings: &Settings, response: &HttpResponse) -> Result<String, AiError> {
        if !is_success(response) {
            return Err(http_error(settings, response));
        }
        let json = json_body(response)?;
        let models = json
            .get("data")
            .and_then(Value::as_array)
            .ok_or(AiError::Malformed)?;
        let listed = models
            .iter()
            .any(|m| m.get("id").and_then(Value::as_str) == Some(settings.model.as_str()));
        let mut text = format!(
            "Connected to {} ({} listed).",
            host_of(settings),
            count_models(models.len())
        );
        if !models.is_empty() && !listed {
            text.push_str(&format!(
                " The model \u{201c}{}\u{201d} is not in that list; check the name.",
                settings.model
            ));
        }
        Ok(text)
    }
}

// ---------------------------------------------------------------------------

/// Anthropic's Messages API.
pub struct Anthropic;

impl Anthropic {
    fn url(settings: &Settings, path: &str) -> String {
        if settings.base_url.ends_with("/v1") {
            format!("{}/{path}", settings.base_url)
        } else {
            format!("{}/v1/{path}", settings.base_url)
        }
    }

    fn headers(key: Option<&ApiKey>) -> Vec<(String, String)> {
        let mut headers = vec![("anthropic-version".to_owned(), ANTHROPIC_VERSION.to_owned())];
        if let Some(key) = key {
            headers.push(("x-api-key".to_owned(), key.expose().to_owned()));
        }
        headers
    }
}

impl Provider for Anthropic {
    fn kind(&self) -> AiProvider {
        AiProvider::Anthropic
    }

    fn needs_key(&self, _base: &BaseUrl) -> bool {
        true
    }

    fn chat_request(&self, settings: &Settings, key: Option<&ApiKey>, prompt: &str) -> HttpRequest {
        let mut body = json!({
            "model": settings.model,
            "max_tokens": settings.max_tokens,
            "messages": [{"role": "user", "content": prompt}],
        });
        if !settings.system_prompt.is_empty() {
            body["system"] = Value::String(settings.system_prompt.clone());
        }
        request(
            settings,
            Method::Post,
            Self::url(settings, "messages"),
            Self::headers(key),
            Some(body),
        )
    }

    fn parse_chat(&self, settings: &Settings, response: &HttpResponse) -> Result<Answer, AiError> {
        if !is_success(response) {
            return Err(http_error(settings, response));
        }
        let json = json_body(response)?;
        let blocks = json
            .get("content")
            .and_then(Value::as_array)
            .ok_or(AiError::Malformed)?;
        let text: String = blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        let cut_off = json.get("stop_reason").and_then(Value::as_str) == Some("max_tokens");
        answer_from(&text, cut_off)
    }

    fn probe_request(&self, settings: &Settings, key: Option<&ApiKey>) -> HttpRequest {
        request(
            settings,
            Method::Get,
            Self::url(settings, "models"),
            Self::headers(key),
            None,
        )
    }

    fn parse_probe(&self, settings: &Settings, response: &HttpResponse) -> Result<String, AiError> {
        if !is_success(response) {
            return Err(http_error(settings, response));
        }
        let json = json_body(response)?;
        let models = json
            .get("data")
            .and_then(Value::as_array)
            .ok_or(AiError::Malformed)?;
        Ok(format!(
            "Connected to {} ({} available).",
            host_of(settings),
            count_models(models.len())
        ))
    }
}

// ---------------------------------------------------------------------------

/// Ollama's native chat API (a model running locally).
pub struct Ollama;

/// Whether the installed tag `tag` is the model `wanted` (`llama3.2` is
/// `llama3.2:latest`).
fn same_model(tag: &str, wanted: &str) -> bool {
    tag == wanted || (!wanted.contains(':') && tag == format!("{wanted}:latest"))
}

impl Provider for Ollama {
    fn kind(&self) -> AiProvider {
        AiProvider::Ollama
    }

    fn needs_key(&self, _base: &BaseUrl) -> bool {
        false
    }

    fn chat_request(
        &self,
        settings: &Settings,
        _key: Option<&ApiKey>,
        prompt: &str,
    ) -> HttpRequest {
        let body = json!({
            "model": settings.model,
            "stream": false,
            "messages": messages(settings, prompt),
            "options": {"num_predict": settings.max_tokens},
        });
        request(
            settings,
            Method::Post,
            format!("{}/api/chat", settings.base_url),
            Vec::new(),
            Some(body),
        )
    }

    fn parse_chat(&self, settings: &Settings, response: &HttpResponse) -> Result<Answer, AiError> {
        if !is_success(response) {
            return Err(http_error(settings, response));
        }
        let json = json_body(response)?;
        let message = json.get("message").ok_or(AiError::Malformed)?;
        let text = message
            .get("content")
            .and_then(Value::as_str)
            .ok_or(AiError::Malformed)?;
        let cut_off = json.get("done_reason").and_then(Value::as_str) == Some("length");
        answer_from(text, cut_off)
    }

    fn probe_request(&self, settings: &Settings, _key: Option<&ApiKey>) -> HttpRequest {
        request(
            settings,
            Method::Get,
            format!("{}/api/tags", settings.base_url),
            Vec::new(),
            None,
        )
    }

    fn parse_probe(&self, settings: &Settings, response: &HttpResponse) -> Result<String, AiError> {
        if !is_success(response) {
            return Err(http_error(settings, response));
        }
        let json = json_body(response)?;
        let models = json
            .get("models")
            .and_then(Value::as_array)
            .ok_or(AiError::Malformed)?;
        let installed = models
            .iter()
            .filter_map(|m| m.get("name").and_then(Value::as_str))
            .any(|tag| same_model(tag, &settings.model));
        if !installed {
            return Err(AiError::ModelNotFound {
                model: settings.model.clone(),
                provider: AiProvider::Ollama,
            });
        }
        Ok(format!(
            "Connected to Ollama at {}; the model \u{201c}{}\u{201d} is installed.",
            host_of(settings),
            settings.model
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(provider: AiProvider, base: &str) -> Settings {
        Settings {
            provider,
            base_url: base.to_owned(),
            model: "test-model".to_owned(),
            system_prompt: "Be brief.".to_owned(),
            max_tokens: 256,
            timeout: Duration::from_secs(30),
        }
    }

    fn reply(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            body: body.as_bytes().to_vec(),
        }
    }

    fn key() -> ApiKey {
        ApiKey::new("sk-test-key-123456").unwrap()
    }

    fn body_of(request: &HttpRequest) -> Value {
        serde_json::from_slice(request.body.as_deref().unwrap()).unwrap()
    }

    fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
        request
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    // --- request shapes -----------------------------------------------------

    #[test]
    fn openai_asks_chat_completions_with_a_bearer_key() {
        let s = settings(AiProvider::OpenAi, "https://api.openai.com/v1");
        let request = OpenAiCompatible.chat_request(&s, Some(&key()), "Why is the sky blue?");
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(
            header(&request, "authorization"),
            Some("Bearer sk-test-key-123456")
        );
        let body = body_of(&request);
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["max_completion_tokens"], 256);
        assert!(body.get("max_tokens").is_none());
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "Be brief.");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "Why is the sky blue?");
        assert!(body.get("stream").is_none());
    }

    #[test]
    fn other_openai_compatible_servers_get_max_tokens_and_no_key_when_there_is_none() {
        let s = settings(AiProvider::OpenAi, "http://localhost:1234/v1");
        let request = OpenAiCompatible.chat_request(&s, None, "hi");
        assert_eq!(request.url, "http://localhost:1234/v1/chat/completions");
        assert!(header(&request, "authorization").is_none());
        let body = body_of(&request);
        assert_eq!(body["max_tokens"], 256);
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn an_empty_system_prompt_sends_no_system_message() {
        let mut s = settings(AiProvider::OpenAi, "http://localhost:1234/v1");
        s.system_prompt.clear();
        let body = body_of(&OpenAiCompatible.chat_request(&s, None, "hi"));
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        let mut a = settings(AiProvider::Anthropic, "https://api.anthropic.com");
        a.system_prompt.clear();
        let body = body_of(&Anthropic.chat_request(&a, Some(&key()), "hi"));
        assert!(body.get("system").is_none());
    }

    #[test]
    fn anthropic_asks_messages_with_its_own_headers() {
        let s = settings(AiProvider::Anthropic, "https://api.anthropic.com");
        let request = Anthropic.chat_request(&s, Some(&key()), "Hello");
        assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(header(&request, "x-api-key"), Some("sk-test-key-123456"));
        assert_eq!(header(&request, "anthropic-version"), Some("2023-06-01"));
        assert!(header(&request, "authorization").is_none());
        let body = body_of(&request);
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["max_tokens"], 256);
        assert_eq!(body["system"], "Be brief.");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
    }

    #[test]
    fn anthropic_does_not_double_the_version_segment() {
        let s = settings(AiProvider::Anthropic, "https://proxy.example.com/v1");
        assert_eq!(
            Anthropic.chat_request(&s, Some(&key()), "x").url,
            "https://proxy.example.com/v1/messages"
        );
        assert_eq!(
            Anthropic.probe_request(&s, Some(&key())).url,
            "https://proxy.example.com/v1/models"
        );
    }

    #[test]
    fn ollama_asks_for_one_complete_answer_and_sends_no_key() {
        let s = settings(AiProvider::Ollama, "http://localhost:11434");
        let request = Ollama.chat_request(&s, Some(&key()), "Hello");
        assert_eq!(request.url, "http://localhost:11434/api/chat");
        assert!(!request.has_credentials());
        let body = body_of(&request);
        assert_eq!(body["stream"], false);
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["options"]["num_predict"], 256);
        assert_eq!(body["messages"][1]["content"], "Hello");
    }

    #[test]
    fn requests_carry_the_timeout_and_a_response_cap() {
        let s = settings(AiProvider::Ollama, "http://localhost:11434");
        let request = Ollama.chat_request(&s, None, "x");
        assert_eq!(request.timeout, Duration::from_secs(30));
        assert_eq!(request.max_response_bytes, MAX_RESPONSE_BYTES);
    }

    #[test]
    fn the_prompt_is_json_encoded_not_spliced_into_the_text() {
        let s = settings(AiProvider::OpenAi, "http://localhost:1234/v1");
        let nasty = "\"}],\"model\":\"evil\",\"x\":[{\"";
        let request = OpenAiCompatible.chat_request(&s, None, nasty);
        let body = body_of(&request);
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["messages"][1]["content"], nasty);
    }

    // --- replies ------------------------------------------------------------

    #[test]
    fn openai_replies_are_read() {
        let s = settings(AiProvider::OpenAi, "https://api.openai.com/v1");
        let ok = reply(
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":" Rayleigh scattering. "},"finish_reason":"stop"}]}"#,
        );
        assert_eq!(
            OpenAiCompatible.parse_chat(&s, &ok).unwrap(),
            Answer {
                text: "Rayleigh scattering.".into(),
                cut_off: false
            }
        );
        let parts = reply(
            200,
            r#"{"choices":[{"message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}]}"#,
        );
        assert_eq!(OpenAiCompatible.parse_chat(&s, &parts).unwrap().text, "ab");
        let long = reply(
            200,
            r#"{"choices":[{"message":{"content":"partial"},"finish_reason":"length"}]}"#,
        );
        assert!(OpenAiCompatible.parse_chat(&s, &long).unwrap().cut_off);
        let reasoning_only = reply(
            200,
            r#"{"choices":[{"message":{"content":null},"finish_reason":"length"}]}"#,
        );
        assert_eq!(
            OpenAiCompatible.parse_chat(&s, &reasoning_only),
            Err(AiError::Empty { cut_off: true })
        );
    }

    #[test]
    fn anthropic_replies_are_read() {
        let s = settings(AiProvider::Anthropic, "https://api.anthropic.com");
        let ok = reply(
            200,
            r#"{"type":"message","content":[{"type":"text","text":"Hello "},{"type":"tool_use","name":"x"},{"type":"text","text":"there"}],"stop_reason":"end_turn"}"#,
        );
        assert_eq!(
            Anthropic.parse_chat(&s, &ok).unwrap(),
            Answer {
                text: "Hello there".into(),
                cut_off: false
            }
        );
        let cut = reply(
            200,
            r#"{"content":[{"type":"text","text":"x"}],"stop_reason":"max_tokens"}"#,
        );
        assert!(Anthropic.parse_chat(&s, &cut).unwrap().cut_off);
    }

    #[test]
    fn ollama_replies_are_read() {
        let s = settings(AiProvider::Ollama, "http://localhost:11434");
        let ok = reply(
            200,
            r#"{"model":"m","message":{"role":"assistant","content":"Hi!"},"done":true,"done_reason":"stop"}"#,
        );
        assert_eq!(Ollama.parse_chat(&s, &ok).unwrap().text, "Hi!");
        let cut = reply(
            200,
            r#"{"message":{"content":"Hi"},"done":true,"done_reason":"length"}"#,
        );
        assert!(Ollama.parse_chat(&s, &cut).unwrap().cut_off);
    }

    #[test]
    fn malformed_replies_are_reported_not_guessed() {
        let cases: [(&dyn Provider, AiProvider); 3] = [
            (&OpenAiCompatible, AiProvider::OpenAi),
            (&Anthropic, AiProvider::Anthropic),
            (&Ollama, AiProvider::Ollama),
        ];
        for (provider, kind) in cases {
            let s = settings(kind, "http://localhost:9");
            for body in [
                "",
                "<html>oops</html>",
                "{",
                "[1,2,3]",
                r#"{"unrelated":true}"#,
            ] {
                assert_eq!(
                    provider.parse_chat(&s, &reply(200, body)),
                    Err(AiError::Malformed),
                    "{kind:?}: {body:?}"
                );
            }
            // Valid JSON of the wrong shape inside a known field.
            let wrong = match kind {
                AiProvider::OpenAi => r#"{"choices":[{"message":{"content":42}}]}"#,
                AiProvider::Anthropic => r#"{"content":"not a list"}"#,
                AiProvider::Ollama => r#"{"message":{"content":42}}"#,
            };
            assert_eq!(
                provider.parse_chat(&s, &reply(200, wrong)),
                Err(AiError::Malformed),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn an_empty_answer_is_an_error() {
        let s = settings(AiProvider::Ollama, "http://localhost:11434");
        assert_eq!(
            Ollama.parse_chat(&s, &reply(200, r#"{"message":{"content":"  \n "}}"#)),
            Err(AiError::Empty { cut_off: false })
        );
    }

    #[test]
    fn statuses_become_friendly_errors() {
        let s = settings(AiProvider::OpenAi, "https://api.openai.com/v1");
        let parse = |status, body| OpenAiCompatible.parse_chat(&s, &reply(status, body));
        assert_eq!(
            parse(
                401,
                r#"{"error":{"message":"Incorrect API key provided: sk-proj-abcdefghijkl"}}"#
            ),
            Err(AiError::Unauthorized {
                provider: AiProvider::OpenAi
            })
        );
        assert!(matches!(parse(403, ""), Err(AiError::Unauthorized { .. })));
        assert_eq!(parse(429, "{}"), Err(AiError::RateLimited));
        assert_eq!(
            parse(503, "overloaded"),
            Err(AiError::Server { status: 503 })
        );
        assert_eq!(
            parse(
                404,
                r#"{"error":{"message":"The model `x` does not exist"}}"#
            ),
            Err(AiError::ModelNotFound {
                model: "test-model".into(),
                provider: AiProvider::OpenAi
            })
        );
        match parse(
            400,
            r#"{"error":{"message":"max_tokens is too large:\n 99999 sk-proj-abcdefghijkl"}}"#,
        ) {
            Err(AiError::Rejected {
                status: 400,
                message,
            }) => {
                assert_eq!(message, "max_tokens is too large: 99999 [key]");
            }
            other => panic!("{other:?}"),
        }
        // A 404 about something else (a wrong base URL) is not "model not found".
        assert!(matches!(
            parse(404, "404 page not found"),
            Err(AiError::Rejected { status: 404, .. })
        ));
        assert!(matches!(
            parse(302, ""),
            Err(AiError::Rejected { status: 302, .. })
        ));
    }

    #[test]
    fn long_server_messages_are_cut() {
        let s = settings(AiProvider::OpenAi, "https://api.openai.com/v1");
        let body = format!(r#"{{"error":{{"message":"{}"}}}}"#, "word ".repeat(200));
        match OpenAiCompatible.parse_chat(&s, &reply(400, &body)) {
            Err(AiError::Rejected { message, .. }) => {
                assert!(message.chars().count() <= MAX_SERVER_MESSAGE_CHARS);
            }
            other => panic!("{other:?}"),
        }
    }

    // --- untrusted text -------------------------------------------------------

    #[test]
    fn answers_are_cleaned_of_control_and_direction_characters() {
        let (text, clipped) =
            clean_answer("a\u{0}b\u{7}c\r\nline\ttab\u{202E}evil\u{2066}x\u{1b}[31m");
        assert_eq!(text, "abc\nline\ttabevilx[31m");
        assert!(!clipped);
        assert_eq!(clean_answer("  \n hello \n ").0, "hello");
        // Emoji sequences keep their joiners.
        assert_eq!(
            clean_answer("\u{1F468}\u{200D}\u{1F469}").0,
            "\u{1F468}\u{200D}\u{1F469}"
        );
    }

    #[test]
    fn answers_are_capped() {
        let (text, clipped) = clean_answer(&"x".repeat(MAX_ANSWER_CHARS + 50));
        assert_eq!(text.chars().count(), MAX_ANSWER_CHARS);
        assert!(clipped);
        let s = settings(AiProvider::Ollama, "http://localhost:11434");
        let body = format!(
            r#"{{"message":{{"content":"{}"}}}}"#,
            "y".repeat(MAX_ANSWER_CHARS + 1)
        );
        assert!(Ollama.parse_chat(&s, &reply(200, &body)).unwrap().cut_off);
    }

    // --- connection tests -------------------------------------------------------

    #[test]
    fn probes_ask_for_the_model_list_and_send_no_prompt() {
        let s = settings(AiProvider::OpenAi, "https://api.openai.com/v1");
        let request = OpenAiCompatible.probe_request(&s, Some(&key()));
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.url, "https://api.openai.com/v1/models");
        assert!(request.body.is_none());
        let s = settings(AiProvider::Ollama, "http://localhost:11434");
        assert_eq!(
            Ollama.probe_request(&s, None).url,
            "http://localhost:11434/api/tags"
        );
    }

    #[test]
    fn probe_replies_are_read() {
        let s = settings(AiProvider::OpenAi, "https://api.openai.com/v1");
        let ok = reply(200, r#"{"data":[{"id":"test-model"},{"id":"other"}]}"#);
        assert_eq!(
            OpenAiCompatible.parse_probe(&s, &ok).unwrap(),
            "Connected to api.openai.com (2 models listed)."
        );
        let missing = reply(200, r#"{"data":[{"id":"other"}]}"#);
        assert!(OpenAiCompatible
            .parse_probe(&s, &missing)
            .unwrap()
            .contains("not in that list"));
        assert_eq!(
            OpenAiCompatible.parse_probe(&s, &reply(401, "{}")),
            Err(AiError::Unauthorized {
                provider: AiProvider::OpenAi
            })
        );
        assert_eq!(
            OpenAiCompatible.parse_probe(&s, &reply(200, "<html>")),
            Err(AiError::Malformed)
        );

        let a = settings(AiProvider::Anthropic, "https://api.anthropic.com");
        assert_eq!(
            Anthropic
                .parse_probe(&a, &reply(200, r#"{"data":[{"id":"x"}]}"#))
                .unwrap(),
            "Connected to api.anthropic.com (1 model available)."
        );
    }

    #[test]
    fn ollama_probe_checks_the_model_is_installed() {
        let s = settings(AiProvider::Ollama, "http://localhost:11434");
        let tags = |name: &str| reply(200, &format!(r#"{{"models":[{{"name":"{name}"}}]}}"#));
        assert!(Ollama.parse_probe(&s, &tags("test-model:latest")).is_ok());
        assert!(Ollama.parse_probe(&s, &tags("test-model")).is_ok());
        assert_eq!(
            Ollama.parse_probe(&s, &tags("something-else:latest")),
            Err(AiError::ModelNotFound {
                model: "test-model".into(),
                provider: AiProvider::Ollama
            })
        );
        let mut tagged = s.clone();
        tagged.model = "test-model:7b".into();
        assert!(Ollama
            .parse_probe(&tagged, &tags("test-model:latest"))
            .is_err());
        assert!(Ollama.parse_probe(&tagged, &tags("test-model:7b")).is_ok());
    }

    #[test]
    fn only_remote_openai_compatible_servers_demand_a_key() {
        let remote = parse_base_url("https://api.openai.com/v1").unwrap();
        let local = parse_base_url("http://localhost:1234/v1").unwrap();
        assert!(OpenAiCompatible.needs_key(&remote));
        assert!(!OpenAiCompatible.needs_key(&local));
        assert!(Anthropic.needs_key(&remote));
        assert!(Anthropic.needs_key(&local));
        assert!(!Ollama.needs_key(&remote));
    }

    #[test]
    fn settings_resolve_defaults_from_the_config() {
        let config = AiConfig {
            provider: AiProvider::Anthropic,
            base_url: "https://proxy.example.com/".into(),
            ..AiConfig::default()
        };
        let s = Settings::from_config(&config);
        assert_eq!(s.base_url, "https://proxy.example.com");
        assert_eq!(s.model, "claude-haiku-4-5");
        assert_eq!(s.timeout, Duration::from_secs(60));
    }
}
