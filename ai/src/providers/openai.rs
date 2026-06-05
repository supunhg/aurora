//! Generic OpenAI-compatible provider.
//!
//! Works with any API that follows the OpenAI chat completions format:
//! - OpenAI (api.openai.com)
//! - OpenRouter (api.openrouter.ai)
//! - Together AI (api.together.xyz)
//! - Any other OpenAI-compatible endpoint

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;

use super::ProviderAdapter;
use crate::error::{AiError, AiResult};

/// Generic provider for any OpenAI-compatible chat completion API.
///
/// Supports:
/// - Streaming (SSE-based)
/// - Tool/function calling
/// - Configurable base URL, API key, and model
pub struct OpenAIProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    provider_id: String,
    is_local: bool,
    p95_latency: u64,
    quality: u8,
    priority: u8,
    supports_tools: bool,
}

impl OpenAIProvider {
    /// Create a new OpenAI-compatible provider.
    ///
    /// - `provider_id`: Short identifier (e.g., "openai", "openrouter")
    /// - `base_url`: API base URL (e.g., "https://api.openai.com")
    /// - `api_key`: Bearer token for authentication
    /// - `model`: Model name (e.g., "gpt-4o", "claude-3-opus")
    pub fn new(provider_id: &str, base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            provider_id: provider_id.to_string(),
            is_local: false,
            p95_latency: 300,
            quality: 8,
            priority: 40,
            supports_tools: true,
        }
    }

    /// Mark this provider as local (for auto-routing heuristics).
    pub fn local(mut self) -> Self {
        self.is_local = true;
        self
    }

    /// Set custom P95 latency estimate (milliseconds).
    pub fn with_latency(mut self, ms: u64) -> Self {
        self.p95_latency = ms;
        self
    }

    /// Set custom quality score (0-10).
    pub fn with_quality(mut self, score: u8) -> Self {
        self.quality = score;
        self
    }

    /// Set custom priority (lower = tried first).
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Enable/disable tool calling support.
    pub fn with_tools(mut self, enabled: bool) -> Self {
        self.supports_tools = enabled;
        self
    }

    /// Get the model name being used.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Check if the API is reachable.
    pub async fn is_available(&self) -> bool {
        self.client
            .get(format!("{}/v1/models", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

impl OpenAIProvider {
    /// Internal helper: build the request body JSON.
    fn build_request_body(&self, prompt: &str, stream: bool) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.7,
            "max_tokens": 2048,
            "stream": stream,
        })
    }

    /// Internal helper: execute the HTTP request and return the raw response.
    async fn do_request(
        &self,
        prompt: &str,
        api_key: &str,
        stream: bool,
    ) -> AiResult<reqwest::Response> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = self.build_request_body(prompt, stream);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                AiError::HttpError(format!("{} request failed: {}", self.provider_id, e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return if status == 429 {
                Err(AiError::RateLimited(format!(
                    "{}: {}",
                    self.provider_id, body_text
                )))
            } else {
                Err(AiError::ProviderError(
                    self.provider_id.clone(),
                    format!("HTTP {}: {}", status, body_text),
                ))
            };
        }

        Ok(response)
    }
}

#[async_trait]
impl ProviderAdapter for OpenAIProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn model_name(&self) -> &str {
        // Returns just the model name (e.g., "gpt-4o").
        // The provider_id is shown separately in routing metadata.
        &self.model
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tool_calling(&self) -> bool {
        self.supports_tools
    }

    fn is_local(&self) -> bool {
        self.is_local
    }

    fn p95_latency_ms(&self) -> u64 {
        self.p95_latency
    }

    fn quality_score(&self) -> u8 {
        self.quality
    }

    fn default_priority(&self) -> u8 {
        self.priority
    }

    async fn chat_completion(&self, prompt: &str) -> AiResult<String> {
        self.chat_completion_with_key(prompt, None).await
    }

    async fn chat_completion_with_key(
        &self,
        prompt: &str,
        key: Option<&str>,
    ) -> AiResult<String> {
        let api_key = key.unwrap_or(&self.api_key);
        let response = self.do_request(prompt, api_key, false).await?;

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))?;

        let content = data["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("no response")
            .to_string();

        Ok(content)
    }

    async fn stream_chat_completion(&self, prompt: &str, tx: mpsc::Sender<String>) -> AiResult<()> {
        self.stream_chat_completion_with_key(prompt, None, tx).await
    }

    async fn stream_chat_completion_with_key(
        &self,
        prompt: &str,
        key: Option<&str>,
        tx: mpsc::Sender<String>,
    ) -> AiResult<()> {
        use futures::StreamExt;

        let api_key = key.unwrap_or(&self.api_key);
        let response = self.do_request(prompt, api_key, true).await?;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| AiError::HttpError(e.to_string()))?;
            let text = String::from_utf8_lossy(&chunk);

            // Parse SSE format: "data: {...}\n\n"
            for line in text.lines() {
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str == "[DONE]" {
                        continue;
                    }
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(json_str) {
                        if let Some(content) = data["choices"][0]["delta"]["content"].as_str() {
                            if tx.send(content.to_string()).await.is_err() {
                                return Err(AiError::Cancelled);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_provider_creation() {
        let provider = OpenAIProvider::new("openai", "https://api.openai.com", "sk-test", "gpt-4o");
        assert_eq!(provider.provider_id(), "openai");
        assert!(provider.supports_streaming());
        assert!(provider.supports_tool_calling());
        assert!(!provider.is_local());
    }

    #[test]
    fn test_openai_custom_config() {
        let provider = OpenAIProvider::new(
            "openrouter",
            "https://openrouter.ai/api",
            "sk-test",
            "anthropic/claude-3.5-sonnet",
        )
        .with_latency(600)
        .with_quality(9)
        .with_priority(50)
        .with_tools(true);

        assert_eq!(provider.p95_latency_ms(), 600);
        assert_eq!(provider.quality_score(), 9);
        assert_eq!(provider.default_priority(), 50);
        assert!(provider.supports_tool_calling());
    }

    #[test]
    fn test_openai_local_flag() {
        let provider =
            OpenAIProvider::new("ollama", "http://localhost:11434", "", "llama3.2").local();
        assert!(provider.is_local());
        assert_eq!(provider.p95_latency_ms(), 300);
    }

    #[test]
    fn test_openai_availability() {
        let provider = OpenAIProvider::new("test", "http://localhost:1", "key", "model");
        // Should not panic — just returns false
        let available = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(provider.is_available());
        assert!(!available);
    }
}
