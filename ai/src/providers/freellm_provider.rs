use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::{AiError, AiResult};
use crate::freellm::{ChatMessage, FreeLlmClient};
use crate::providers::ProviderAdapter;

/// AI provider backed by the FreeLLMAPI sidecar.
///
/// Wraps `FreeLlmClient` and implements the `ProviderAdapter` trait so it can
/// be registered with the `AIRouter`. The sidecar handles multi-provider
/// fallback, rate limiting, and health checking internally.
pub struct FreeLlmProvider {
    client: Arc<Mutex<FreeLlmClient>>,
    model: String,
}

impl FreeLlmProvider {
    /// Create a new provider from an existing `FreeLlmClient`.
    pub fn new(client: FreeLlmClient, model: &str) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            model: model.to_string(),
        }
    }

    /// Create a provider pointing at the default localhost sidecar.
    pub fn localhost(model: &str) -> Self {
        Self::new(FreeLlmClient::localhost(), model)
    }

    /// Create a provider from sidecar URL and API key.
    pub fn from_sidecar(base_url: &str, api_key: &str, model: &str) -> Self {
        Self::new(FreeLlmClient::new(base_url, api_key), model)
    }
}

#[async_trait]
impl ProviderAdapter for FreeLlmProvider {
    fn provider_id(&self) -> &str {
        "freellmapi"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tool_calling(&self) -> bool {
        true
    }

    fn default_priority(&self) -> u8 {
        5 // Very high priority — use the sidecar first
    }

    fn p95_latency_ms(&self) -> u64 {
        300 // Depends on which provider the sidecar routes to
    }

    fn is_local(&self) -> bool {
        false // The sidecar routes to cloud providers by default
    }

    fn quality_score(&self) -> u8 {
        8 // High quality — aggregates multiple top-tier providers
    }

    async fn chat_completion(&self, prompt: &str) -> AiResult<String> {
        let client = self.client.lock().await;
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt.to_string(),
        }];

        let response = client.chat_completion(&self.model, messages).await?;

        response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| {
                AiError::ProviderError("freellmapi".into(), "No content in response".into())
            })
    }

    async fn stream_chat_completion(
        &self,
        prompt: &str,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> AiResult<()> {
        let client = self.client.lock().await;
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: prompt.to_string(),
        }];

        client
            .chat_completion_stream(&self.model, messages, tx)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_basics() {
        let provider = FreeLlmProvider::localhost("auto");
        assert_eq!(provider.provider_id(), "freellmapi");
        assert_eq!(provider.model_name(), "auto");
        assert!(provider.supports_streaming());
        assert!(provider.supports_tool_calling());
        assert_eq!(provider.default_priority(), 5);
        assert!(!provider.is_local());
        assert_eq!(provider.quality_score(), 8);
    }

    #[test]
    fn test_provider_custom_model() {
        let provider = FreeLlmProvider::localhost("gemini-2.5-flash");
        assert_eq!(provider.model_name(), "gemini-2.5-flash");
    }

    #[test]
    fn test_provider_from_url() {
        let provider = FreeLlmProvider::from_sidecar(
            "http://localhost:4000",
            "test-key",
            "groq/llama-3.3-70b",
        );
        assert_eq!(provider.model_name(), "groq/llama-3.3-70b");
    }
}
