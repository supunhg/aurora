//! MockCloudProvider — simulates a rate-limited cloud API for testing the fallback chain.

use async_trait::async_trait;
use std::time::Duration;

use super::ProviderAdapter;
use crate::error::{AiError, AiResult};

struct MockRateState {
    count: u32,
    max_per_minute: u32,
    last_reset: std::time::Instant,
}

/// Mock cloud provider that simulates rate limiting for testing the fallback chain.
///
/// Allows a configurable number of requests per minute, then returns
/// `AiError::RateLimited` for subsequent requests until the window resets.
pub struct MockCloudProvider {
    name: &'static str,
    rate_limit: std::sync::Mutex<MockRateState>,
}

impl MockCloudProvider {
    /// Create a new mock provider with a given name and max requests per minute.
    pub fn new(name: &'static str, max_per_minute: u32) -> Self {
        Self {
            name,
            rate_limit: std::sync::Mutex::new(MockRateState {
                count: 0,
                max_per_minute,
                last_reset: std::time::Instant::now(),
            }),
        }
    }
}

#[async_trait]
impl ProviderAdapter for MockCloudProvider {
    fn provider_id(&self) -> &str {
        self.name
    }
    fn model_name(&self) -> &str {
        self.name
    }
    fn p95_latency_ms(&self) -> u64 {
        350
    }
    fn quality_score(&self) -> u8 {
        5
    }

    async fn chat_completion(&self, prompt: &str) -> AiResult<String> {
        let allowed = {
            let mut state = self.rate_limit.lock().unwrap();
            if state.last_reset.elapsed() > Duration::from_secs(60) {
                state.count = 0;
                state.last_reset = std::time::Instant::now();
            }
            if state.count >= state.max_per_minute {
                false
            } else {
                state.count += 1;
                true
            }
        };

        if !allowed {
            return Err(AiError::RateLimited(self.name.to_string()));
        }

        tokio::time::sleep(Duration::from_millis(350)).await;
        Ok(format!("[{}] Response to '{}'", self.name, prompt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn test_mock_cloud_resets_after_window() {
        let provider = MockCloudProvider::new("test_reset", 2);
        assert!(provider.chat_completion("r1").await.is_ok());
        assert!(provider.chat_completion("r2").await.is_ok());
        assert!(provider.chat_completion("r3").await.is_err());
    }
}
