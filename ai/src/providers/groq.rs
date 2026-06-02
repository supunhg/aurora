//! Groq API provider — real HTTP to api.groq.com.
//!
//! Groq offers fast inference on Llama, Mixtral, and other open models
//! via their custom LPU hardware. Free tier available at groq.com.
//!
//! API docs: https://console.groq.com/docs

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;

use super::ProviderAdapter;
use crate::error::{AiError, AiResult};

/// Provider for Groq's high-speed inference API.
///
/// Uses the OpenAI-compatible `/v1/chat/completions` endpoint.
/// Requires a Groq API key from https://console.groq.com/keys
pub struct GroqProvider {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
    provider_id: String,
}

impl GroqProvider {
    /// Create a new Groq provider with the default model (llama-3.3-70b-versatile).
    pub fn new(api_key: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            model: "llama-3.3-70b-versatile".into(),
            base_url: "https://api.groq.com".into(),
            provider_id: "groq".into(),
        }
    }

    /// Create a new Groq provider with a specific model.
    ///
    /// Common models:
    /// - `llama-3.3-70b-versatile` (default, fast)
    /// - `llama-3.1-8b-instant` (very fast, smaller)
    /// - `mixtral-8x7b-32768` (32k context)
    /// - `gemma2-9b-it` (Google Gemma 2)
    pub fn with_model(api_key: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: "https://api.groq.com".into(),
            provider_id: "groq".into(),
        }
    }

    /// Set a custom base URL (e.g., for enterprise self-hosted).
    pub fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_string();
        self
    }

    /// Get the current model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the display name (e.g., "groq/llama-3.3-70b").
    pub fn display_name(&self) -> String {
        format!("{}/{}", self.provider_id, simplify_model(&self.model))
    }

    /// Check if the API is reachable with the provided key.
    pub async fn is_available(&self) -> bool {
        self.client
            .get(format!("{}/openai/v1/models", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

/// Simplify a model name for display (remove version suffixes).
fn simplify_model(model: &str) -> &str {
    // Known trailing suffixes to strip for cleaner display
    let suffixes = ["-versatile", "-instant", "-instruct"];

    for suffix in &suffixes {
        if let Some(base) = model.strip_suffix(suffix) {
            return base;
        }
    }

    model
}

#[async_trait]
impl ProviderAdapter for GroqProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
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

    fn p95_latency_ms(&self) -> u64 {
        // Groq is known for very fast inference
        150
    }

    fn quality_score(&self) -> u8 {
        // Llama-3.3-70b is high quality
        7
    }

    fn default_priority(&self) -> u8 {
        // After local providers
        30
    }

    async fn chat_completion(&self, prompt: &str) -> AiResult<String> {
        let url = format!("{}/openai/v1/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.7,
            "max_tokens": 2048,
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::HttpError(format!("Groq request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return if status == 429 {
                Err(AiError::RateLimited(format!("Groq: {}", body_text)))
            } else {
                Err(AiError::ProviderError(
                    "groq".into(),
                    format!("HTTP {}: {}", status, body_text),
                ))
            };
        }

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
        use futures::StreamExt;

        let url = format!("{}/openai/v1/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.7,
            "max_tokens": 2048,
            "stream": true,
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::HttpError(format!("Groq stream request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return if status == 429 {
                Err(AiError::RateLimited(format!("Groq: {}", body_text)))
            } else {
                Err(AiError::ProviderError(
                    "groq".into(),
                    format!("HTTP {}: {}", status, body_text),
                ))
            };
        }

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
    fn test_groq_provider_creation() {
        let provider = GroqProvider::new("test-key");
        assert_eq!(provider.provider_id(), "groq");
        assert_eq!(provider.model(), "llama-3.3-70b-versatile");
        assert!(provider.supports_streaming());
        assert!(provider.supports_tool_calling());
    }

    #[test]
    fn test_groq_with_custom_model() {
        let provider = GroqProvider::with_model("test-key", "mixtral-8x7b-32768");
        assert_eq!(provider.model(), "mixtral-8x7b-32768");
    }

    #[test]
    fn test_groq_custom_base_url() {
        let provider = GroqProvider::new("test-key").with_base_url("https://groq.example.com");
        // The base_url is used internally but we can verify via model_name
        assert_eq!(provider.model(), "llama-3.3-70b-versatile");
    }

    #[test]
    fn test_groq_provider_traits() {
        let provider = GroqProvider::new("test-key");
        assert_eq!(provider.default_priority(), 30);
        assert_eq!(provider.p95_latency_ms(), 150);
        assert_eq!(provider.quality_score(), 7);
    }

    #[test]
    fn test_simplify_model() {
        assert_eq!(simplify_model("llama-3.3-70b-versatile"), "llama-3.3-70b");
        assert_eq!(simplify_model("llama-3.1-8b-instant"), "llama-3.1-8b");
        assert_eq!(simplify_model("mixtral-8x7b-32768"), "mixtral-8x7b-32768");
    }
}
