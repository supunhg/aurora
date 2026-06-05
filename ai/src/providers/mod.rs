//! AI provider adapters — unified interface for chat completion APIs.
//!
//! This module defines the [`ProviderAdapter`] trait and provides implementations
//! for various AI backends:
//!
//! | Provider | Type | Auth | Status |
//! |----------|------|------|--------|
//! | [`OllamaProvider`] | Local (HTTP) | None | ✅ Ready |
//! | [`GroqProvider`] | Cloud (HTTP) | Bearer token | ✅ Ready |
//! | [`OpenAIProvider`] | Cloud (HTTP) | Bearer token | ✅ Ready |
//! | [`LocalProvider`] | Simulated | None | ✅ Test/mock |
//! | [`MockCloudProvider`] | Simulated | None | ✅ Test/mock |
//! | [`FreeLlmProvider`] | Sidecar (HTTP) | Bearer token | 🟡 Deprecating |

// Public provider modules (exported for use)
pub mod groq;
pub mod ollama;
pub mod openai;

// Internal modules (not re-exported, but types are re-exported below)
mod local;
mod mock;

// Sidecar provider — kept for backward compatibility
pub mod freellm_provider;

// Re-export all provider types
pub use groq::GroqProvider;
pub use local::LocalProvider;
pub use mock::MockCloudProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAIProvider;

use async_trait::async_trait;

use crate::error::{AiError, AiResult};

/// Provider adapter trait for AI chat completions.
///
/// Each provider implements this trait to expose a unified interface.
/// The router uses this trait to dispatch requests through the fallback chain.
///
/// # Implementing a new provider
///
/// 1. Create a new file in `ai/src/providers/` (e.g., `anthropic.rs`)
/// 2. Implement the [`ProviderAdapter`] trait for your provider struct
/// 3. Add `pub mod my_provider;` and `pub use my_provider::MyProvider;` in this module
/// 4. Register it in [`crate::router::AIRouter::register_provider()`]
///
/// See `openai.rs` for a complete implementation example.
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// Unique provider identifier (e.g., "groq", "ollama", "openai").
    fn provider_id(&self) -> &str;

    /// Human-readable model name for display (e.g., "groq/llama-3.3-70b").
    /// This is used in routing metadata and status bar display.
    fn model_name(&self) -> &str;

    /// Whether this provider supports streaming responses (SSE).
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Whether this provider supports tool/function calling.
    fn supports_tool_calling(&self) -> bool {
        false
    }

    /// Default priority (lower = tried first). Range: 0 (highest) to 255 (lowest).
    ///
    /// Recommended values:
    /// - 0-20: Local providers (Ollama, llama.cpp)
    /// - 30-60: Fast free-tier cloud (Groq, Cerebras)
    /// - 70-100: Quality cloud (OpenAI, Anthropic, Gemini)
    /// - 200+: Simulated/mock providers for testing
    fn default_priority(&self) -> u8 {
        100
    }

    /// P95 latency estimate in milliseconds (used for auto-routing heuristics).
    fn p95_latency_ms(&self) -> u64 {
        500
    }

    /// Whether this is a local (on-device) provider. Used by auto-routing to
    /// prefer local models for latency-sensitive requests like inline completions.
    fn is_local(&self) -> bool {
        false
    }

    /// Quality score 0-10 (used for auto-routing heuristics).
    /// Higher scores are preferred for quality-sensitive requests (chat, refactor).
    fn quality_score(&self) -> u8 {
        5
    }

    /// Execute a chat completion request.
    ///
    /// The `prompt` is a simple string. For multi-message conversations,
    /// the caller should format messages into a single prompt or extend
    /// this trait in the future.
    async fn chat_completion(&self, prompt: &str) -> AiResult<String>;

    /// Execute a chat completion with an explicit API key override.
    ///
    /// If `key` is `Some`, the provider must use that key for the request.
    /// If `key` is `None`, the provider falls back to its default/baked-in key.
    ///
    /// Default implementation delegates to [`chat_completion`].
    /// Providers that support key rotation should override this method.
    async fn chat_completion_with_key(
        &self,
        prompt: &str,
        key: Option<&str>,
    ) -> AiResult<String> {
        let _ = key;
        self.chat_completion(prompt).await
    }

    /// Stream a chat completion response through the provided channel.
    ///
    /// Default implementation collects the full response from [`chat_completion`]
    /// and sends it as a single chunk. Providers that support true streaming
    /// should override this method.
    async fn stream_chat_completion(
        &self,
        prompt: &str,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> AiResult<()> {
        let response = self.chat_completion(prompt).await?;
        tx.send(response).await.map_err(|_| AiError::Cancelled)?;
        Ok(())
    }

    /// Stream a chat completion with an explicit API key override.
    ///
    /// Default implementation delegates to [`stream_chat_completion`].
    async fn stream_chat_completion_with_key(
        &self,
        prompt: &str,
        key: Option<&str>,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> AiResult<()> {
        let _ = key;
        self.stream_chat_completion(prompt, tx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_provider_trait() {
        let provider = LocalProvider::new();
        assert!(provider.is_local());
        assert_eq!(provider.provider_id(), "local");
        assert!(!provider.supports_streaming());

        let result = provider.chat_completion("hello").await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("local/llama"));
    }

    #[tokio::test]
    async fn test_ollama_trait() {
        let provider = OllamaProvider::new("test-model");
        assert!(provider.is_local());
        assert_eq!(provider.provider_id(), "ollama");
        assert!(provider.supports_streaming());
    }

    #[tokio::test]
    async fn test_groq_trait() {
        let provider = GroqProvider::new("test-key");
        assert!(!provider.is_local());
        assert_eq!(provider.provider_id(), "groq");
        assert!(provider.supports_streaming());
    }

    #[tokio::test]
    async fn test_openai_trait() {
        let provider = OpenAIProvider::new("test-provider", "https://api.test.com", "key", "model");
        assert!(!provider.is_local());
        assert_eq!(provider.provider_id(), "test-provider");
        assert!(provider.supports_streaming());
        assert!(provider.supports_tool_calling());
    }

    #[tokio::test]
    async fn test_mock_cloud_rate_limit() {
        let provider = MockCloudProvider::new("test", 3);

        for i in 0..3 {
            let result = provider.chat_completion(&format!("req {}", i)).await;
            assert!(result.is_ok(), "Request {} should succeed", i);
        }

        let result = provider.chat_completion("req 4").await;
        assert!(result.is_err(), "4th request should be rate limited");
    }
}
