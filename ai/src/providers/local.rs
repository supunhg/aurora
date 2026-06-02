//! LocalProvider — simulated local inference provider.
//!
//! In production, this should be replaced with llama-cpp-rs or candle
//! for actual on-device inference. Currently simulates a response with
//! configurable latency.

use async_trait::async_trait;
use std::time::Duration;

use super::ProviderAdapter;
use crate::error::AiResult;

/// Simulated local model provider.
///
/// Produces a fake response after a configurable delay.
/// Used as a fallback when no real providers are available.
pub struct LocalProvider {
    name: &'static str,
    latency_ms: u64,
}

impl LocalProvider {
    pub fn new() -> Self {
        Self {
            name: "local/llama",
            latency_ms: 250,
        }
    }

    /// Set a custom simulated latency.
    pub fn with_latency(latency_ms: u64) -> Self {
        Self {
            name: "local/llama",
            latency_ms,
        }
    }
}

impl Default for LocalProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderAdapter for LocalProvider {
    fn provider_id(&self) -> &str {
        "local"
    }
    fn model_name(&self) -> &str {
        self.name
    }
    fn is_local(&self) -> bool {
        true
    }
    fn p95_latency_ms(&self) -> u64 {
        self.latency_ms
    }
    fn quality_score(&self) -> u8 {
        6
    }
    fn default_priority(&self) -> u8 {
        10
    }

    async fn chat_completion(&self, prompt: &str) -> AiResult<String> {
        tokio::time::sleep(Duration::from_millis(self.latency_ms)).await;
        Ok(format!(
            "[local/llama] Simulated response to: '{}' (took {}ms)",
            prompt, self.latency_ms
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_provider() {
        let provider = LocalProvider::new();
        let result = provider.chat_completion("hello").await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("local/llama"));
    }

    #[test]
    fn test_local_traits() {
        let local = LocalProvider::new();
        assert!(local.is_local());
        assert!(!local.supports_streaming());
        assert_eq!(local.default_priority(), 10);
    }
}
