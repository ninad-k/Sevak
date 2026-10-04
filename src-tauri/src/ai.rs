//! The commands behind Settings > AI assistant: the write-only API key field and
//! "Test connection".
//!
//! The key goes one way: the field sends it to [`ai_set_key`], which stores it
//! (see `sevak_plugins::ai::keys`), and nothing here ever returns it, logs it or
//! puts it in an error. [`ai_key_status`] only says where a key comes from.

use serde::Serialize;
use sevak_core::{AiConfig, AiProvider};
use sevak_platform::secret;
use sevak_plugins::ai::keys::{KeySource, KeyStatus, KeyStore, StoredOrEnv};
use sevak_plugins::ai::{ApiKey, Assistant};

/// What Settings shows next to one provider's key field.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct KeyInfo {
    /// `none`, `stored`, `env` or `unreadable`.
    pub state: &'static str,
    /// The environment variable in use (`env`) or accepted (`none`).
    pub env_var: Option<&'static str>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct AiKeyStatus {
    pub openai: KeyInfo,
    pub anthropic: KeyInfo,
    /// How a stored key is kept, in words (shown under the field).
    pub protection: &'static str,
}

fn info(keys: &StoredOrEnv, provider: AiProvider) -> KeyInfo {
    match keys.status(provider) {
        KeyStatus::Missing => KeyInfo {
            state: "none",
            env_var: provider.key_env_var(),
        },
        KeyStatus::Unreadable => KeyInfo {
            state: "unreadable",
            env_var: provider.key_env_var(),
        },
        KeyStatus::InUse(KeySource::Stored) => KeyInfo {
            state: "stored",
            env_var: provider.key_env_var(),
        },
        KeyStatus::InUse(KeySource::Environment(var)) => KeyInfo {
            state: "env",
            env_var: Some(var),
        },
    }
}

fn status() -> AiKeyStatus {
    let keys = StoredOrEnv::system();
    AiKeyStatus {
        openai: info(&keys, AiProvider::OpenAi),
        anthropic: info(&keys, AiProvider::Anthropic),
        protection: secret::protection().describe(),
    }
}

/// A provider that takes an API key, by its id.
fn keyed_provider(id: &str) -> Result<AiProvider, String> {
    match AiProvider::from_id(id) {
        Some(provider) if provider.key_env_var().is_some() => Ok(provider),
        _ => Err("That provider does not use an API key.".to_owned()),
    }
}

fn store() -> Result<KeyStore, String> {
    KeyStore::default_location().ok_or_else(|| "Sevak's data folder could not be found.".to_owned())
}

#[tauri::command]
pub async fn ai_key_status() -> Result<AiKeyStatus, String> {
    tauri::async_runtime::spawn_blocking(status)
        .await
        .map_err(|err| format!("the check did not finish: {err}"))
}

/// Stores the key typed into the (write-only) field. It takes effect at once;
/// it is not part of the settings form's Save.
#[tauri::command]
pub async fn ai_set_key(provider: String, key: String) -> Result<AiKeyStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider = keyed_provider(&provider)?;
        let key = ApiKey::new(&key).ok_or_else(|| {
            "That does not look like an API key (no spaces or line breaks, at most 512 characters)."
                .to_owned()
        })?;
        store()?
            .set(provider, &key)
            .map_err(|err| format!("could not save the key: {err}"))?;
        tracing::info!(provider = provider.id(), "an AI API key was saved");
        Ok(status())
    })
    .await
    .map_err(|err| format!("saving did not finish: {err}"))?
}

#[tauri::command]
pub async fn ai_clear_key(provider: String) -> Result<AiKeyStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider = keyed_provider(&provider)?;
        store()?
            .clear(provider)
            .map_err(|err| format!("could not remove the key: {err}"))?;
        tracing::info!(provider = provider.id(), "an AI API key was removed");
        Ok(status())
    })
    .await
    .map_err(|err| format!("removing did not finish: {err}"))?
}

/// "Test connection": checks the address, the key and the model with the
/// settings as they are in the form (saved or not). No question is sent; the
/// request lists the provider's models. It runs only when the button is pressed.
#[tauri::command]
pub async fn ai_test_connection(config: AiConfig) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(problem) = config.problem() {
            return Err(problem);
        }
        Assistant::new(&config.normalized())
            .test_connection()
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| format!("the test did not finish: {err}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_providers_with_keys_can_have_one_stored() {
        assert_eq!(keyed_provider("openai"), Ok(AiProvider::OpenAi));
        assert_eq!(keyed_provider("anthropic"), Ok(AiProvider::Anthropic));
        assert!(keyed_provider("ollama").is_err());
        assert!(keyed_provider("nonsense").is_err());
    }

    #[test]
    fn the_status_never_carries_a_key() {
        let json = serde_json::to_string(&status()).unwrap();
        assert!(!json.contains("sk-"));
        for field in ["state", "env_var", "protection"] {
            assert!(json.contains(field), "{json}");
        }
    }
}
