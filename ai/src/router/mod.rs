use std::sync::Arc;

use crate::error::{AiError, AiResult};
use crate::health::HealthMonitor;
use crate::keystore::{EphemeralKeyStore, KeyId};
use crate::providers::ProviderAdapter;
use crate::ratelimit::{RateKey, RateLimitLedger};
use serde::{Deserialize, Serialize};

/// An AI request to the router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIRequest {
    pub prompt: String,
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub model_hint: Option<String>,
    #[serde(default)]
    pub stream: bool,
}

/// Metadata about how a request was routed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingMetadata {
    pub routed_via: String,
    pub fallback_attempts: u8,
    pub total_latency_ms: u64,
}

/// A provider registered with the router.
struct ProviderEntry {
    adapter: Arc<dyn ProviderAdapter>,
    key_id: Option<KeyId>,
}

/// The main AI request router with fallback chain logic.
pub struct AIRouter {
    providers: Vec<ProviderEntry>,
    rate_ledger: Arc<RateLimitLedger>,
    health_monitor: Option<Arc<HealthMonitor>>,
    key_store: Arc<EphemeralKeyStore>,
}

impl AIRouter {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            rate_ledger: Arc::new(RateLimitLedger::new()),
            health_monitor: None,
            key_store: Arc::new(EphemeralKeyStore::new()),
        }
    }

    /// Attach a rate limit ledger (reuses shared state).
    pub fn with_rate_ledger(mut self, ledger: Arc<RateLimitLedger>) -> Self {
        self.rate_ledger = ledger;
        self
    }

    /// Attach a health monitor.
    pub fn with_health_monitor(mut self, monitor: Arc<HealthMonitor>) -> Self {
        self.health_monitor = Some(monitor);
        self
    }

    /// Register a provider in the fallback chain.
    pub fn register_provider(&mut self, provider: Arc<dyn ProviderAdapter>) {
        self.providers.push(ProviderEntry {
            adapter: provider,
            key_id: None,
        });
    }

    /// Register a provider with an API key.
    pub fn register_provider_with_key(
        &mut self,
        provider: Arc<dyn ProviderAdapter>,
        api_key: &str,
    ) -> KeyId {
        let key_id = self.key_store.add_key(provider.provider_id(), api_key);
        self.providers.push(ProviderEntry {
            adapter: provider,
            key_id: Some(key_id.clone()),
        });
        key_id
    }

    /// Route a request through the fallback chain.
    /// Tries each provider in registration order, skipping unhealthy or rate-limited ones.
    pub async fn route(&self, req: AIRequest) -> AiResult<RoutingMetadata> {
        if self.providers.is_empty() {
            return Err(AiError::NoProviders);
        }

        let start = std::time::Instant::now();
        let mut fallback_attempts = 0u8;
        let mut last_error = AiError::AllProvidersFailed("no providers tried".into());

        for entry in &self.providers {
            let provider_id = entry.adapter.provider_id();
            let model_name = entry.adapter.model_name();

            // Check health monitor (if attached)
            if let Some(ref monitor) = self.health_monitor {
                if let Some(ref key_id) = entry.key_id {
                    let health = monitor.key_health(&key_id.0);
                    if !health.is_usable() {
                        tracing::debug!(
                            "[router] Skipping {} ({}) — health state: {}",
                            provider_id,
                            model_name,
                            health.label()
                        );
                        fallback_attempts += 1;
                        last_error = AiError::ProviderError(
                            provider_id.into(),
                            format!("health state: {}", health.label()),
                        );
                        continue;
                    }
                }
            }

            // Check rate limits
            if let Some(ref key_id) = entry.key_id {
                let rate_key = RateKey {
                    provider: provider_id.to_string(),
                    model: model_name.to_string(),
                    key_id: key_id.0.clone(),
                };
                if !self.rate_ledger.can_request(&rate_key, 100) {
                    tracing::debug!(
                        "[router] Skipping {} ({}) — rate limited",
                        provider_id,
                        model_name
                    );
                    fallback_attempts += 1;
                    last_error = AiError::RateLimited(format!("{}/{}", provider_id, model_name));
                    continue;
                }
            }

            // Attempt the request
            match entry.adapter.chat_completion(&req.prompt).await {
                Ok(response) => {
                    let latency = start.elapsed().as_millis() as u64;

                    // Record success in rate ledger
                    if let Some(ref key_id) = entry.key_id {
                        let rate_key = RateKey {
                            provider: provider_id.to_string(),
                            model: model_name.to_string(),
                            key_id: key_id.0.clone(),
                        };
                        self.rate_ledger
                            .record_request(&rate_key, response.len(), true);
                    }

                    return Ok(RoutingMetadata {
                        routed_via: model_name.to_string(),
                        fallback_attempts,
                        total_latency_ms: latency,
                    });
                }
                Err(e) => {
                    tracing::warn!("[router] {} ({}) failed: {}", provider_id, model_name, e);
                    fallback_attempts += 1;
                    last_error = e;

                    // Record failure in rate ledger
                    if let Some(ref key_id) = entry.key_id {
                        let rate_key = RateKey {
                            provider: provider_id.to_string(),
                            model: model_name.to_string(),
                            key_id: key_id.0.clone(),
                        };
                        self.rate_ledger.record_request(&rate_key, 0, false);

                        // If rate limited, set cooldown
                        if matches!(&last_error, AiError::RateLimited(_)) {
                            self.rate_ledger
                                .set_cooldown(&rate_key, std::time::Duration::from_secs(10));
                        }
                    }
                }
            }
        }

        Err(last_error)
    }

    /// Get the number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Get the key store reference.
    pub fn key_store(&self) -> &EphemeralKeyStore {
        &self.key_store
    }
}

impl Default for AIRouter {
    fn default() -> Self {
        Self::new()
    }
}
