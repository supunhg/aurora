//! Ollama provider — local inference via HTTP to localhost:11434.
//!
//! Uses Ollama's OpenAI-compatible `/v1/chat/completions` endpoint.
//! No API key required. Models must be pulled via `ollama pull <model>` first.

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;

use super::ProviderAdapter;
use crate::error::{AiError, AiResult};

/// Provider for Ollama local inference.
///
/// Connects to `http://localhost:11434` (configurable).
/// Uses the OpenAI-compatible `/v1/chat/completions` endpoint.
pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    /// Create a new Ollama provider pointing at localhost:11434 with the given model.
    pub fn new(model: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: "http://localhost:11434".into(),
            model: model.to_string(),
        }
    }

    /// Create a new Ollama provider with a custom base URL.
    pub fn with_url(base_url: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
        }
    }

    /// Set the model to use.
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the current model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Check if Ollama is running and accessible.
    pub async fn is_available(&self) -> bool {
        self.client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// List models available in Ollama.
    pub async fn list_models(&self) -> AiResult<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .map_err(|e| AiError::HttpError(format!("Ollama connection failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(AiError::ProviderError(
                "ollama".into(),
                format!("HTTP {}", response.status()),
            ));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))?;

        let models = data["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    /// Auto-detect the best model available. Prefers smaller/faster models.
    pub async fn auto_detect_model(&self) -> Option<String> {
        let models = self.list_models().await.ok()?;
        if models.is_empty() {
            return None;
        }

        // Preference order: fast general-purpose models first
        let preferred = [
            "llama3.2",
            "llama3.1",
            "llama3",
            "qwen2.5",
            "qwen2",
            "mistral",
            "codellama",
            "deepseek-coder",
            "phi",
            "phi3",
        ];

        for name in &preferred {
            if models.iter().any(|m| m.contains(name)) {
                return Some(name.to_string());
            }
        }

        // Fall back to first available model
        Some(models[0].clone())
    }
}

#[async_trait]
impl ProviderAdapter for OllamaProvider {
    fn provider_id(&self) -> &str {
        "ollama"
    }

    fn model_name(&self) -> &str {
        // Return e.g. "ollama/llama3.2" or just the model name
        &self.model
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tool_calling(&self) -> bool {
        // Ollama added tool calling support in v0.5.0+
        true
    }

    fn is_local(&self) -> bool {
        true
    }

    fn p95_latency_ms(&self) -> u64 {
        // Local inference is slower than cloud typically
        500
    }

    fn quality_score(&self) -> u8 {
        // Ollama quality depends on the model pulled
        6
    }

    fn default_priority(&self) -> u8 {
        // Local providers should be tried first
        5
    }

    async fn chat_completion(&self, prompt: &str) -> AiResult<String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "stream": false,
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::HttpError(format!("Ollama request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Err(AiError::ProviderError(
                "ollama".into(),
                format!("HTTP {}: {}", status, body_text),
            ));
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

        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "stream": true,
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::HttpError(format!("Ollama stream request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            return Err(AiError::ProviderError(
                "ollama".into(),
                format!("HTTP {}: {}", status, body_text),
            ));
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
    fn test_ollama_provider_creation() {
        let provider = OllamaProvider::new("llama3.2");
        assert_eq!(provider.provider_id(), "ollama");
        assert_eq!(provider.base_url(), "http://localhost:11434");
        assert_eq!(provider.model(), "llama3.2");
        assert!(provider.is_local());
        assert!(provider.supports_streaming());
    }

    #[test]
    fn test_ollama_custom_url() {
        let provider = OllamaProvider::with_url("http://192.168.1.100:11434", "mistral");
        assert_eq!(provider.base_url(), "http://192.168.1.100:11434");
        assert_eq!(provider.model(), "mistral");
    }

    #[test]
    fn test_ollama_with_model_chain() {
        let provider = OllamaProvider::new("llama3.2").with_model("codellama");
        assert_eq!(provider.model(), "codellama");
    }

    #[test]
    fn test_provider_trait_methods() {
        let provider = OllamaProvider::new("llama3.2");
        assert_eq!(provider.default_priority(), 5);
        assert!(provider.supports_tool_calling());
        assert!(provider.supports_streaming());
    }
}
