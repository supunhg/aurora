pub mod events;

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::mpsc;

use crate::error::{AiError, AiResult};
use crate::health::HealthMonitor;
use crate::keystore::{EphemeralKeyStore, KeyId, ProviderKeyPool, SelectedKey};
use crate::providers::ProviderAdapter;
use crate::ratelimit::{RateKey, RateLimitLedger};
use crate::router::events::{KeyRotationEvent, RotationReason};
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

/// The main AI request router with fallback chain logic and per-provider key rotation.
pub struct AIRouter {
    providers: Vec<ProviderEntry>,
    rate_ledger: Arc<RateLimitLedger>,
    health_monitor: Option<Arc<HealthMonitor>>,
    key_store: Arc<EphemeralKeyStore>,
    /// Per-provider key pools for multi-key rotation.
    key_pools: DashMap<String, ProviderKeyPool>,
    /// Event sender for UI notifications (key rotation, exhaustion, etc.).
    event_tx: Option<mpsc::UnboundedSender<KeyRotationEvent>>,
}

impl AIRouter {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            rate_ledger: Arc::new(RateLimitLedger::new()),
            health_monitor: None,
            key_store: Arc::new(EphemeralKeyStore::new()),
            key_pools: DashMap::new(),
            event_tx: None,
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

    /// Attach an event sender for key rotation / exhaustion notifications.
    pub fn with_event_sender(mut self, tx: mpsc::UnboundedSender<KeyRotationEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Register a provider in the fallback chain.
    pub fn register_provider(&mut self, provider: Arc<dyn ProviderAdapter>) {
        self.providers.push(ProviderEntry {
            adapter: provider,
            key_id: None,
        });
    }

    /// Register a provider with an API key (legacy single-key mode).
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

    /// Register a key pool for a provider.
    /// When the router targets this provider, it will rotate through keys in the pool.
    pub fn register_key_pool(&self, pool: ProviderKeyPool) {
        let provider_id = pool.provider_id().to_string();
        self.key_pools.insert(provider_id, pool);
    }

    /// Check if a key pool exists for a provider.
    pub fn has_key_pool(&self, provider_id: &str) -> bool {
        self.key_pools.contains_key(provider_id)
    }

    /// Inspect all registered key pools read-only.
    ///
    /// The callback receives the provider ID and a read-only reference to the pool.
    /// Do NOT call mutating methods (add_key, remove_key, mark_*) inside the callback
    /// — doing so could deadlock because the DashMap read guard is held.
    pub fn inspect_key_pools<F>(&self, mut f: F)
    where
        F: FnMut(&str, &ProviderKeyPool),
    {
        for entry in self.key_pools.iter() {
            f(entry.key(), entry.value());
        }
    }

    /// Route a request through the fallback chain.
    ///
    /// For each provider:
    /// 1. If a key pool exists, try each key in the pool until one succeeds.
    /// 2. If no pool exists, fall back to legacy single-key behavior.
    /// 3. If all keys in a provider's pool are exhausted, move to the next provider.
    /// 4. If all providers are exhausted, emit an event and return an error.
    ///
    /// Pass `stream_tx: Some(tx)` to route a streaming request; chunks will be
    /// forwarded to the provided sender in real time.
    ///
    /// **Streaming retry behavior:** When streaming, a failed key does not retry
    /// within the same pool — the router advances to the next provider to avoid
    /// mixing partial output from different keys.
    pub async fn route(&self, req: AIRequest, stream_tx: Option<mpsc::Sender<String>>) -> AiResult<RoutingMetadata> {
        if self.providers.is_empty() {
            return Err(AiError::NoProviders);
        }

        let start = std::time::Instant::now();
        let mut fallback_attempts = 0u8;

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
                        tracing::debug!("[router] {} health check failed", provider_id);
                        continue;
                    }
                }
            }

            // --- KEY POOL BRANCH: rotate through multiple keys for this provider ---
            if let Some(pool) = self.key_pools.get(provider_id) {
                let max_attempts = pool.usable_count().max(1);
                let mut attempt_count = 0usize;
                let mut previous_key: Option<(KeyId, String)> = None;
                let mut rotation_reason: Option<RotationReason> = None;

                while attempt_count < max_attempts {
                    let selected = pool.select_key();

                    if selected.is_none() {
                        // All keys in this provider's pool are exhausted.
                        // ProviderExhausted is emitted once after the loop below.
                        tracing::debug!(
                            "[router] All keys exhausted for provider {}",
                            provider_id
                        );
                        fallback_attempts += 1;
                        tracing::debug!("[router] {} pool exhausted", provider_id);
                        break; // move to next provider
                    }

                    let key: SelectedKey = selected.unwrap();
                    attempt_count += 1;

                    // Emit rotation event when we switched to a different key
                    if let Some((ref prev_id, ref prev_label)) = previous_key {
                        if prev_id.0 != key.key_id.0 {
                            if let Some(reason) = rotation_reason {
                                self.emit_event(KeyRotationEvent::KeyRotated {
                                    provider: provider_id.to_string(),
                                    from_key_id: prev_id.0.clone(),
                                    from_key_label: prev_label.clone(),
                                    to_key_id: key.key_id.0.clone(),
                                    to_key_label: key.label.clone(),
                                    reason,
                                });
                            }
                        }
                    }
                    previous_key = Some((key.key_id.clone(), key.label.clone()));

                    let rate_key = RateKey {
                        provider: provider_id.to_string(),
                        model: model_name.to_string(),
                        key_id: key.key_id.0.clone(),
                    };

                    // Ensure counters exist with the correct per-key quota
                    self.rate_ledger.ensure_counters(&rate_key, &key.quota);

                    // Check rate limits for this specific key
                    if !self.rate_ledger.can_request(&rate_key, 100) {
                        tracing::debug!(
                            "[router] Skipping {} key {} — rate limited",
                            provider_id,
                            key.label
                        );
                        pool.mark_rate_limited(&key.key_id, Duration::from_secs(60));
                        fallback_attempts += 1;
                        rotation_reason = Some(RotationReason::RateLimited);
                        tracing::debug!("[router] {} rate limited (ledger)", provider_id);
                        continue; // try next key in pool
                    }

                    // Attempt the request with this specific key
                    let provider_result: AiResult<Option<String>> = if let Some(ref tx) = stream_tx {
                        let tx_attempt = tx.clone();
                        entry.adapter
                            .stream_chat_completion_with_key(&req.prompt, Some(&key.api_key), tx_attempt)
                            .await
                            .map(|_| None)
                    } else {
                        entry.adapter
                            .chat_completion_with_key(&req.prompt, Some(&key.api_key))
                            .await
                            .map(Some)
                    };

                    match provider_result {
                        Ok(maybe_response) => {
                            let latency = start.elapsed().as_millis() as u64;

                            // Record success
                            // TODO: response.len() is a rough character proxy for tokens.
                            // Replace with actual token counting (e.g., tiktoken) for accurate
                            // TPM/TPD tracking.
                            let token_estimate = maybe_response.map(|r| r.len()).unwrap_or(500);
                            self.rate_ledger
                                .record_request(&rate_key, token_estimate, true);
                            pool.update_latency(&key.key_id, latency);

                            // Proactive quota monitoring & events
                            if self.rate_ledger.is_critical(&rate_key) {
                                if let Some((dim, pct)) =
                                    self.rate_ledger.hottest_dimension(&rate_key)
                                {
                                    self.emit_event(KeyRotationEvent::KeyCritical {
                                        provider: provider_id.to_string(),
                                        key_id: key.key_id.0.clone(),
                                        key_label: key.label.clone(),
                                        percent_used: pct,
                                        dimension: dim.to_string(),
                                    });
                                    pool.mark_approaching_limit(&key.key_id, pct, dim);
                                }
                            } else if self.rate_ledger.is_approaching_limit(&rate_key) {
                                if let Some((dim, pct)) =
                                    self.rate_ledger.hottest_dimension(&rate_key)
                                {
                                    self.emit_event(KeyRotationEvent::KeyApproachingLimit {
                                        provider: provider_id.to_string(),
                                        key_id: key.key_id.0.clone(),
                                        key_label: key.label.clone(),
                                        percent_used: pct,
                                        dimension: dim.to_string(),
                                    });
                                    pool.mark_approaching_limit(&key.key_id, pct, dim);
                                }
                            }

                            // Emit usage update
                            if let Some((dim, pct)) =
                                self.rate_ledger.hottest_dimension(&rate_key)
                            {
                                self.emit_event(KeyRotationEvent::UsageUpdated {
                                    provider: provider_id.to_string(),
                                    key_id: key.key_id.0.clone(),
                                    key_label: key.label.clone(),
                                    percent_used: pct,
                                    dimension: dim.to_string(),
                                });
                            }

                            return Ok(RoutingMetadata {
                                routed_via: format!(
                                    "{}/{} (key: {})",
                                    provider_id, model_name, key.label
                                ),
                                fallback_attempts,
                                total_latency_ms: latency,
                            });
                        }
                        Err(AiError::RateLimited(ref msg)) => {
                            tracing::warn!(
                                "[router] {} key {} rate limited: {}",
                                provider_id,
                                key.label,
                                msg
                            );
                            fallback_attempts += 1;
                            pool.mark_rate_limited(&key.key_id, Duration::from_secs(60));
                            rotation_reason = Some(RotationReason::RateLimited);
                            tracing::debug!("[router] {} rate limited (HTTP 429)", provider_id);
                            // When streaming, don't retry another key in the same pool
                            // because partial chunks may have already been sent.
                            if stream_tx.is_some() {
                                break;
                            }
                        }
                        Err(AiError::ProviderError(_, ref msg))
                            if msg.contains("401")
                                || msg.contains("403")
                                || msg.contains("Unauthorized")
                                || msg.contains("unauthorized") =>
                        {
                            tracing::error!(
                                "[router] {} key {} invalid: {}",
                                provider_id,
                                key.label,
                                msg
                            );
                            fallback_attempts += 1;
                            pool.mark_invalid(&key.key_id);
                            rotation_reason = Some(RotationReason::InvalidKey);
                            self.emit_event(KeyRotationEvent::KeyInvalidated {
                                provider: provider_id.to_string(),
                                key_id: key.key_id.0.clone(),
                                key_label: key.label.clone(),
                                error: msg.clone(),
                            });
                            tracing::debug!("[router] {} key invalid", provider_id);
                            if stream_tx.is_some() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "[router] {} key {} failed: {}",
                                provider_id,
                                key.label,
                                e
                            );
                            fallback_attempts += 1;
                            rotation_reason = Some(RotationReason::TransientError);
                            tracing::debug!("[router] {} transient error: {}", provider_id, e);
                            // When streaming, don't retry another key in the same pool
                            // because partial chunks may have already been sent.
                            if stream_tx.is_some() {
                                break;
                            }
                            // Non-streaming: skip this key and try the next one.
                        }
                    }
                }

                // If we reach here, the pool did not produce a successful response
                // (all keys failed, or streaming broke, or no keys were selectable).
                self.emit_event(KeyRotationEvent::ProviderExhausted {
                    provider: provider_id.to_string(),
                });

                // move to the next provider in the fallback chain.
                continue;
            }

            // --- LEGACY SINGLE-KEY BRANCH ---
            // Check rate limits for the single baked-in key
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
                    tracing::debug!("[router] {} rate limited (legacy)", provider_id);
                    continue;
                }
            }

            // Attempt the request with the provider's default/baked-in key
            let provider_result: AiResult<Option<String>> = if let Some(ref tx) = stream_tx {
                let tx_attempt = tx.clone();
                entry.adapter
                    .stream_chat_completion(&req.prompt, tx_attempt)
                    .await
                    .map(|_| None)
            } else {
                entry.adapter
                    .chat_completion(&req.prompt)
                    .await
                    .map(Some)
            };

            match provider_result {
                Ok(maybe_response) => {
                    let latency = start.elapsed().as_millis() as u64;

                    // Record success in rate ledger
                    if let Some(ref key_id) = entry.key_id {
                        let rate_key = RateKey {
                            provider: provider_id.to_string(),
                            model: model_name.to_string(),
                            key_id: key_id.0.clone(),
                        };
                        let token_estimate = maybe_response.map(|r| r.len()).unwrap_or(500);
                        self.rate_ledger
                            .record_request(&rate_key, token_estimate, true);
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
                    // Record failure in rate ledger
                    if let Some(ref key_id) = entry.key_id {
                        let rate_key = RateKey {
                            provider: provider_id.to_string(),
                            model: model_name.to_string(),
                            key_id: key_id.0.clone(),
                        };
                        self.rate_ledger.record_request(&rate_key, 0, false);

                        // If rate limited, set cooldown
                        if matches!(e, AiError::RateLimited(_)) {
                            self.rate_ledger
                                .set_cooldown(&rate_key, Duration::from_secs(10));
                        }
                    }
                }
            }
        }

        // All providers exhausted
        self.emit_event(KeyRotationEvent::AllKeysExhausted);
        Err(AiError::AllKeysExhausted)
    }

    /// Get the number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Get the number of registered key pools.
    pub fn key_pool_count(&self) -> usize {
        self.key_pools.len()
    }

    /// Get the key store reference.
    pub fn key_store(&self) -> &EphemeralKeyStore {
        &self.key_store
    }

    /// Get the rate ledger reference.
    pub fn rate_ledger(&self) -> &RateLimitLedger {
        &self.rate_ledger
    }

    /// Route a streaming request through the fallback chain.
    ///
    /// Convenience wrapper around [`Self::route`] that passes a stream sender.
    /// Response chunks are forwarded to `tx` in real time.
    /// Returns routing metadata once the stream completes (or errors).
    pub async fn route_stream(
        &self,
        req: AIRequest,
        tx: mpsc::Sender<String>,
    ) -> AiResult<RoutingMetadata> {
        self.route(req, Some(tx)).await
    }

    /// Fire a key rotation event (fire-and-forget).
    fn emit_event(&self, event: KeyRotationEvent) {
        if let Some(ref tx) = self.event_tx {
            let _ = tx.send(event);
        }
    }
}

impl Default for AIRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{AiError, AiResult};
    use crate::keystore::quota::ProviderQuota;
    use crate::keystore::{
        DecryptedApiKey, KeyHealth, KeyId, KeySource, PoolKeyEntry, ProviderKeyPool,
    };
    use crate::providers::ProviderAdapter;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A test mock provider that returns configurable results per API key.
    struct BehaviorMock {
        name: &'static str,
        behaviors: Mutex<HashMap<String, Result<String, AiError>>>,
    }

    impl BehaviorMock {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                behaviors: Mutex::new(HashMap::new()),
            }
        }

        fn set(&self, key: &str, result: Result<String, AiError>) {
            self.behaviors.lock().unwrap().insert(key.to_string(), result);
        }

        fn lookup(&self, key: Option<&str>) -> AiResult<String> {
            let key_str = key.unwrap_or("default");
            let behaviors = self.behaviors.lock().unwrap();
            match behaviors.get(key_str) {
                Some(result) => result.clone(),
                None => Ok(format!("[{}] ok", self.name)),
            }
        }
    }

    #[async_trait]
    impl ProviderAdapter for BehaviorMock {
        fn provider_id(&self) -> &str {
            self.name
        }
        fn model_name(&self) -> &str {
            self.name
        }

        async fn chat_completion(&self, _prompt: &str) -> AiResult<String> {
            self.lookup(None)
        }

        async fn chat_completion_with_key(
            &self,
            _prompt: &str,
            key: Option<&str>,
        ) -> AiResult<String> {
            self.lookup(key)
        }

        async fn stream_chat_completion_with_key(
            &self,
            _prompt: &str,
            key: Option<&str>,
            tx: mpsc::Sender<String>,
        ) -> AiResult<()> {
            let response = self.lookup(key)?;
            tx.send(response).await.map_err(|_| AiError::Cancelled)?;
            Ok(())
        }
    }

    fn make_pool_key(label: &str, key_value: &str) -> PoolKeyEntry {
        PoolKeyEntry {
            key_id: KeyId(format!("id-{}", label)),
            api_key: DecryptedApiKey {
                value: key_value.to_string(),
                provider: "test".to_string(),
            },
            label: label.to_string(),
            source: KeySource::UiPanel,
            health: parking_lot::RwLock::new(KeyHealth::Healthy),
            quota: ProviderQuota::unlimited(),
            p95_latency_ms: parking_lot::RwLock::new(100),
        }
    }

    fn collect_events(rx: &mut mpsc::UnboundedReceiver<KeyRotationEvent>) -> Vec<KeyRotationEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    fn setup_router_with_pool(
        provider: Arc<dyn ProviderAdapter>,
        pool: ProviderKeyPool,
    ) -> (AIRouter, mpsc::UnboundedReceiver<KeyRotationEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut router = AIRouter::new().with_event_sender(tx);
        router.register_key_pool(pool);
        router.register_provider(provider);
        (router, rx)
    }

    fn setup_router_with_fallback(
        provider_a: Arc<dyn ProviderAdapter>,
        pool_a: ProviderKeyPool,
        provider_b: Arc<dyn ProviderAdapter>,
        pool_b: Option<ProviderKeyPool>,
    ) -> (AIRouter, mpsc::UnboundedReceiver<KeyRotationEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut router = AIRouter::new().with_event_sender(tx);
        router.register_key_pool(pool_a);
        router.register_provider(provider_a);
        if let Some(pool) = pool_b {
            router.register_key_pool(pool);
        }
        router.register_provider(provider_b);
        (router, rx)
    }

    // -----------------------------------------------------------------------
    // Edge case: all keys in a pool are rate-limited → fallback to next provider
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_all_keys_rate_limited_fallback() {
        let mock_a = Arc::new(BehaviorMock::new("provider_a"));
        mock_a.set("key-a1", Err(AiError::RateLimited("a1".into())));
        mock_a.set("key-a2", Err(AiError::RateLimited("a2".into())));

        let mock_b = Arc::new(BehaviorMock::new("provider_b"));

        let pool_a = ProviderKeyPool::new("provider_a");
        pool_a.add_key(make_pool_key("a1", "key-a1"));
        pool_a.add_key(make_pool_key("a2", "key-a2"));

        let pool_b = ProviderKeyPool::new("provider_b");
        pool_b.add_key(make_pool_key("b1", "key-b1"));

        let (router, mut rx) =
            setup_router_with_fallback(mock_a, pool_a, mock_b, Some(pool_b));

        let req = AIRequest {
            prompt: "hello".into(),
            conversation_id: None,
            model_hint: None,
            stream: false,
        };

        let result = router.route(req, None).await;
        assert!(result.is_ok(), "Should fallback to provider_b");
        let meta = result.unwrap();
        assert!(
            meta.routed_via.contains("provider_b"),
            "Should route via provider_b"
        );
        assert!(meta.fallback_attempts >= 2, "Should have recorded fallback attempts");

        let events = collect_events(&mut rx);
        let has_exhausted = events.iter().any(|e| {
            matches!(e, KeyRotationEvent::ProviderExhausted { provider } if provider == "provider_a")
        });
        assert!(has_exhausted, "Should emit ProviderExhausted for provider_a");
    }

    // -----------------------------------------------------------------------
    // Edge case: one key invalid → rotate to second key and succeed
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_one_key_invalid_rotates_to_second() {
        let mock = Arc::new(BehaviorMock::new("test_prov"));
        mock.set("key-bad", Err(AiError::ProviderError(
            "401".into(),
            "Unauthorized: invalid key".into(),
        )));
        mock.set("key-good", Ok("success".into()));

        let pool = ProviderKeyPool::new("test_prov");
        pool.add_key(make_pool_key("bad", "key-bad"));
        pool.add_key(make_pool_key("good", "key-good"));

        let (router, mut rx) = setup_router_with_pool(mock, pool);

        let req = AIRequest {
            prompt: "hello".into(),
            conversation_id: None,
            model_hint: None,
            stream: false,
        };

        let result = router.route(req, None).await;
        assert!(result.is_ok(), "Should succeed after rotating to good key");
        let meta = result.unwrap();
        assert!(meta.routed_via.contains("good"));
        assert_eq!(meta.fallback_attempts, 1);

        let events = collect_events(&mut rx);
        let rotated = events.iter().any(|e| matches!(e, KeyRotationEvent::KeyRotated {
            from_key_label,
            to_key_label,
            reason: RotationReason::InvalidKey,
            ..
        } if from_key_label == "bad" && to_key_label == "good"));
        assert!(rotated, "Should emit KeyRotated with InvalidKey reason");

        let invalidated = events.iter().any(|e| matches!(e, KeyRotationEvent::KeyInvalidated {
            key_label,
            ..
        } if key_label == "bad"));
        assert!(invalidated, "Should emit KeyInvalidated for bad key");
    }

    // -----------------------------------------------------------------------
    // Edge case: transient error → retry next key in pool
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_transient_error_retries_next_key() {
        let mock = Arc::new(BehaviorMock::new("test_prov"));
        mock.set("key-flaky", Err(AiError::ProviderError(
            "503".into(),
            "Service Unavailable".into(),
        )));
        mock.set("key-stable", Ok("stable-response".into()));

        let pool = ProviderKeyPool::new("test_prov");
        pool.add_key(make_pool_key("flaky", "key-flaky"));
        pool.add_key(make_pool_key("stable", "key-stable"));

        let (router, mut rx) = setup_router_with_pool(mock, pool);

        let req = AIRequest {
            prompt: "hello".into(),
            conversation_id: None,
            model_hint: None,
            stream: false,
        };

        let result = router.route(req, None).await;
        assert!(result.is_ok(), "Should succeed on second key after transient failure");
        let meta = result.unwrap();
        assert!(meta.routed_via.contains("stable"));
        assert_eq!(meta.fallback_attempts, 1);

        let events = collect_events(&mut rx);
        let rotated = events.iter().any(|e| matches!(e, KeyRotationEvent::KeyRotated {
            from_key_label,
            to_key_label,
            reason: RotationReason::TransientError,
            ..
        } if from_key_label == "flaky" && to_key_label == "stable"));
        assert!(rotated, "Should emit KeyRotated with TransientError reason");
    }

    // -----------------------------------------------------------------------
    // Edge case: provider fallback chain when pool exhausted
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_provider_fallback_chain() {
        let mock_a = Arc::new(BehaviorMock::new("provider_a"));
        mock_a.set("key-a1", Err(AiError::ProviderError("500".into(), "boom".into())));
        mock_a.set("key-a2", Err(AiError::ProviderError("500".into(), "boom".into())));

        let mock_b = Arc::new(BehaviorMock::new("provider_b"));
        mock_b.set("key-b1", Ok("from-b".into()));

        let pool_a = ProviderKeyPool::new("provider_a");
        pool_a.add_key(make_pool_key("a1", "key-a1"));
        pool_a.add_key(make_pool_key("a2", "key-a2"));

        let pool_b = ProviderKeyPool::new("provider_b");
        pool_b.add_key(make_pool_key("b1", "key-b1"));

        let (router, mut rx) =
            setup_router_with_fallback(mock_a, pool_a, mock_b, Some(pool_b));

        let req = AIRequest {
            prompt: "hello".into(),
            conversation_id: None,
            model_hint: None,
            stream: false,
        };

        let result = router.route(req, None).await;
        assert!(result.is_ok(), "Should fallback to provider_b");
        let meta = result.unwrap();
        assert!(meta.routed_via.contains("provider_b"));
        assert!(meta.fallback_attempts >= 2);

        let events = collect_events(&mut rx);
        let exhausted = events.iter().any(|e| matches!(e, KeyRotationEvent::ProviderExhausted {
            provider,
        } if provider == "provider_a"));
        assert!(exhausted, "Should emit ProviderExhausted for provider_a");
    }

    // -----------------------------------------------------------------------
    // Edge case: streaming mode does NOT retry within same pool
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_streaming_breaks_on_failure_no_intra_pool_retry() {
        let mock_a = Arc::new(BehaviorMock::new("provider_a"));
        mock_a.set("key-a1", Err(AiError::ProviderError("503".into(), "transient".into())));
        mock_a.set("key-a2", Ok("should-not-reach".into()));

        let mock_b = Arc::new(BehaviorMock::new("provider_b"));
        mock_b.set("key-b1", Ok("from-b".into()));

        let pool_a = ProviderKeyPool::new("provider_a");
        pool_a.add_key(make_pool_key("a1", "key-a1"));
        pool_a.add_key(make_pool_key("a2", "key-a2"));

        let pool_b = ProviderKeyPool::new("provider_b");
        pool_b.add_key(make_pool_key("b1", "key-b1"));

        let (router, mut rx) =
            setup_router_with_fallback(mock_a, pool_a, mock_b, Some(pool_b));

        let (stream_tx, mut stream_rx) = mpsc::channel(10);
        let req = AIRequest {
            prompt: "hello".into(),
            conversation_id: None,
            model_hint: None,
            stream: true,
        };

        let result = router.route(req, Some(stream_tx)).await;
        assert!(result.is_ok(), "Should fallback to provider_b in streaming mode");
        let meta = result.unwrap();
        assert!(meta.routed_via.contains("provider_b"));

        // Verify we received provider_b's chunk, not provider_a key2's chunk
        let mut chunks = Vec::new();
        while let Ok(chunk) = stream_rx.try_recv() {
            chunks.push(chunk);
        }
        assert!(
            chunks.iter().any(|c| c.contains("from-b")),
            "Should receive provider_b's stream chunk"
        );
        assert!(
            !chunks.iter().any(|c| c.contains("should-not-reach")),
            "Should NOT receive provider_a key2's stream chunk"
        );

        let events = collect_events(&mut rx);
        let exhausted = events.iter().any(|e| matches!(e, KeyRotationEvent::ProviderExhausted {
            provider,
        } if provider == "provider_a"));
        assert!(exhausted, "Should emit ProviderExhausted after streaming break");
    }

    // -----------------------------------------------------------------------
    // KeyRotated event carries correct from/to IDs and reason
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_key_rotated_event_fields_are_accurate() {
        let mock = Arc::new(BehaviorMock::new("test_prov"));
        mock.set("key-alpha", Err(AiError::RateLimited("alpha".into())));
        mock.set("key-beta", Ok("beta-response".into()));

        let pool = ProviderKeyPool::new("test_prov");
        pool.add_key(make_pool_key("alpha", "key-alpha"));
        pool.add_key(make_pool_key("beta", "key-beta"));

        let (router, mut rx) = setup_router_with_pool(mock, pool);

        let req = AIRequest {
            prompt: "hello".into(),
            conversation_id: None,
            model_hint: None,
            stream: false,
        };

        let result = router.route(req, None).await;
        assert!(result.is_ok());

        let events = collect_events(&mut rx);
        let rotated = events.iter().find_map(|e| match e {
            KeyRotationEvent::KeyRotated {
                from_key_id,
                from_key_label,
                to_key_id,
                to_key_label,
                reason,
                ..
            } => Some((from_key_id.clone(), from_key_label.clone(), to_key_id.clone(), to_key_label.clone(), *reason)),
            _ => None,
        });

        assert!(
            rotated.is_some(),
            "Should emit exactly one KeyRotated event"
        );
        let (from_id, from_label, to_id, to_label, reason) = rotated.unwrap();
        assert_eq!(from_label, "alpha");
        assert_eq!(to_label, "beta");
        assert!(from_id.contains("id-alpha"));
        assert!(to_id.contains("id-beta"));
        assert_eq!(reason, RotationReason::RateLimited);
    }

    // -----------------------------------------------------------------------
    // All providers exhausted → AllKeysExhausted error
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_all_providers_exhausted() {
        let mock_a = Arc::new(BehaviorMock::new("provider_a"));
        mock_a.set("key-a1", Err(AiError::RateLimited("a1".into())));

        let mock_b = Arc::new(BehaviorMock::new("provider_b"));
        mock_b.set("key-b1", Err(AiError::RateLimited("b1".into())));

        let pool_a = ProviderKeyPool::new("provider_a");
        pool_a.add_key(make_pool_key("a1", "key-a1"));

        let pool_b = ProviderKeyPool::new("provider_b");
        pool_b.add_key(make_pool_key("b1", "key-b1"));

        let (router, mut rx) =
            setup_router_with_fallback(mock_a, pool_a, mock_b, Some(pool_b));

        let req = AIRequest {
            prompt: "hello".into(),
            conversation_id: None,
            model_hint: None,
            stream: false,
        };

        let result = router.route(req, None).await;
        assert!(
            matches!(result, Err(AiError::AllKeysExhausted)),
            "Should return AllKeysExhausted when every provider fails"
        );

        let events = collect_events(&mut rx);
        let all_exhausted = events.iter().any(|e| {
            matches!(e, KeyRotationEvent::AllKeysExhausted)
        });
        assert!(all_exhausted, "Should emit AllKeysExhausted event");
    }

    // -----------------------------------------------------------------------
    // Legacy single-key provider still works when no pool is registered
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_legacy_single_key_provider_without_pool() {
        let mock = Arc::new(BehaviorMock::new("legacy_prov"));
        mock.set("default", Ok("legacy-ok".into()));

        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut router = AIRouter::new().with_event_sender(tx);
        // Legacy registration: no pool, provider has its own baked-in key behavior
        router.register_provider(mock);

        let req = AIRequest {
            prompt: "hello".into(),
            conversation_id: None,
            model_hint: None,
            stream: false,
        };

        let result = router.route(req, None).await;
        assert!(result.is_ok(), "Legacy provider should succeed");
        let meta = result.unwrap();
        assert!(meta.routed_via.contains("legacy_prov"));
        assert_eq!(meta.fallback_attempts, 0);

        // No events expected for a clean single-key success path
        let events = collect_events(&mut rx);
        assert!(events.is_empty());
    }
}
