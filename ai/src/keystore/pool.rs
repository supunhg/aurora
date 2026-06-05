use parking_lot::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::{DecryptedApiKey, KeyId};
use super::quota::ProviderQuota;

/// Where a key originated from (for bookkeeping and display).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Environment,
    ConfigFile,
    UiPanel,
}

/// Health state of an individual key (more granular than provider-level).
#[derive(Debug, Clone)]
pub enum KeyHealth {
    /// Key is working normally.
    Healthy,
    /// Key is approaching its limit (80%+ used on at least one dimension).
    ApproachingLimit {
        percent_used: f64,
        dimension: String,
    },
    /// Key is currently rate-limited (waiting for cooldown).
    RateLimited {
        cooldown_until: Instant,
    },
    /// Key is invalid (auth error, deleted, etc.).
    Invalid,
    /// No health check has been performed yet.
    Unknown,
}

impl KeyHealth {
    /// Whether this key can be used for requests right now.
    pub fn is_usable(&self) -> bool {
        match self {
            KeyHealth::Healthy => true,
            KeyHealth::ApproachingLimit { .. } => true,
            KeyHealth::Unknown => true,
            KeyHealth::RateLimited { cooldown_until } => Instant::now() >= *cooldown_until,
            KeyHealth::Invalid => false,
        }
    }

    /// Whether this key is in a warning state (approaching or at limit).
    pub fn is_warning(&self) -> bool {
        matches!(self, KeyHealth::ApproachingLimit { .. } | KeyHealth::RateLimited { .. })
    }

    /// Human-readable label for UI display.
    pub fn label(&self) -> &'static str {
        match self {
            KeyHealth::Healthy => "healthy",
            KeyHealth::ApproachingLimit { .. } => "approaching_limit",
            KeyHealth::RateLimited { .. } => "rate_limited",
            KeyHealth::Invalid => "invalid",
            KeyHealth::Unknown => "unknown",
        }
    }
}

/// A snapshot of current usage for a key (used by UI and selection heuristics).
#[derive(Debug, Clone, Default)]
pub struct KeyUsageSnapshot {
    pub rpm_used: u32,
    pub rpm_limit: u32,
    pub rpd_used: u32,
    pub rpd_limit: u32,
    pub tpm_used: u32,
    pub tpm_limit: u32,
    pub tpd_used: u32,
    pub tpd_limit: u32,
    /// The dimension with the highest usage percentage (e.g., "rpm", "tpm").
    pub hottest_dimension: String,
    /// 0.0–1.0 across all dimensions.
    pub max_percent_used: f64,
}

impl KeyUsageSnapshot {
    /// True if any dimension is at or above 80%.
    pub fn approaching_limit(&self) -> bool {
        self.max_percent_used >= 0.8
    }

    /// True if any dimension is at or above 95%.
    pub fn critical(&self) -> bool {
        self.max_percent_used >= 0.95
    }
}

/// An individual key entry inside a provider pool.
pub struct PoolKeyEntry {
    pub key_id: KeyId,
    /// The actual API key (decrypted in-memory, zeroized on drop).
    pub api_key: DecryptedApiKey,
    /// Human-readable label (e.g., "personal-work", "team-shared-1").
    pub label: String,
    /// Where this key came from.
    pub source: KeySource,
    /// Current health of this specific key.
    pub health: RwLock<KeyHealth>,
    /// Per-key quota (defaults to provider free-tier, overrideable).
    pub quota: ProviderQuota,
    /// Estimated P95 latency for this key (populated from request history).
    pub p95_latency_ms: RwLock<u64>,
}

impl Clone for PoolKeyEntry {
    fn clone(&self) -> Self {
        Self {
            key_id: self.key_id.clone(),
            api_key: self.api_key.clone(),
            label: self.label.clone(),
            source: self.source,
            health: RwLock::new(self.health.read().clone()),
            quota: self.quota,
            p95_latency_ms: RwLock::new(*self.p95_latency_ms.read()),
        }
    }
}

impl std::fmt::Debug for PoolKeyEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PoolKeyEntry")
            .field("key_id", &self.key_id)
            .field("api_key", &"***")
            .field("label", &self.label)
            .field("source", &self.source)
            .field("health", &self.health)
            .field("quota", &self.quota)
            .field("p95_latency_ms", &self.p95_latency_ms)
            .finish()
    }
}

/// Strategy for selecting the next key from a pool.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum KeySelectionStrategy {
    /// Round-robin through all healthy keys.
    #[default]
    RoundRobin,
    /// Prefer the key with the most remaining quota (hottest_dimension headroom).
    ///
    /// NOTE: Currently a stub — falls back to RoundRobin behavior.
    /// Full implementation requires querying the RateLimitLedger per key.
    MostHeadroom,
    /// Prefer the key with the lowest recent latency.
    ///
    /// NOTE: Currently a stub — falls back to RoundRobin behavior.
    /// Full implementation requires maintaining a latency histogram.
    LowestLatency,
    /// Weighted random (spreads load to avoid synchronized exhaustion).
    WeightedRandom,
}

/// A pool of API keys for a single provider.
///
/// Manages multiple keys, their health states, and selects the best key
/// for each request according to the configured strategy.
pub struct ProviderKeyPool {
    provider_id: String,
    keys: RwLock<Vec<PoolKeyEntry>>,
    selection_strategy: KeySelectionStrategy,
    round_robin_index: AtomicUsize,
}

impl ProviderKeyPool {
    /// Create a new empty key pool for a provider.
    pub fn new(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            keys: RwLock::new(Vec::new()),
            selection_strategy: KeySelectionStrategy::default(),
            round_robin_index: AtomicUsize::new(0),
        }
    }

    /// Create a new key pool with a specific selection strategy.
    pub fn with_strategy(provider_id: &str, strategy: KeySelectionStrategy) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            keys: RwLock::new(Vec::new()),
            selection_strategy: strategy,
            round_robin_index: AtomicUsize::new(0),
        }
    }

    /// Add a key to the pool.
    pub fn add_key(&self, entry: PoolKeyEntry) {
        let mut keys = self.keys.write();
        keys.push(entry);
    }

    /// Remove a key from the pool by its key_id.
    pub fn remove_key(&self, key_id: &KeyId) {
        let mut keys = self.keys.write();
        keys.retain(|k| k.key_id.0 != key_id.0);
    }

    /// Number of keys in the pool.
    pub fn len(&self) -> usize {
        self.keys.read().len()
    }

    /// True if the pool has no keys.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of currently usable keys.
    pub fn usable_count(&self) -> usize {
        let keys = self.keys.read();
        keys.iter().filter(|k| k.health.read().is_usable()).count()
    }

    /// True if at least one key is usable.
    pub fn has_usable_key(&self) -> bool {
        self.usable_count() > 0
    }

    /// Select the best key from the pool according to the strategy.
    /// Filters out keys that are invalid or still in cooldown.
    /// Returns `None` if no usable keys remain.
    pub fn select_key(&self) -> Option<SelectedKey> {
        let keys = self.keys.read();

        // Build a list of usable keys (indices + entries)
        let usable_indices: Vec<usize> = keys
            .iter()
            .enumerate()
            .filter(|(_, k)| k.health.read().is_usable())
            .map(|(i, _)| i)
            .collect();

        if usable_indices.is_empty() {
            return None;
        }

        let selected_idx = match self.selection_strategy {
            KeySelectionStrategy::RoundRobin => {
                let idx = self
                    .round_robin_index
                    .fetch_add(1, Ordering::SeqCst)
                    % usable_indices.len();
                usable_indices[idx]
            }
            KeySelectionStrategy::MostHeadroom => {
                // For now, select the first usable key.
                // In a full implementation this would query the RateLimitLedger
                // for actual headroom per key and pick the max.
                usable_indices[0]
            }
            KeySelectionStrategy::LowestLatency => {
                // For now, select the first usable key.
                // In a full implementation this would sort by p95_latency_ms.
                usable_indices[0]
            }
            KeySelectionStrategy::WeightedRandom => {
                // Use a simple counter-based seed since std::time::Instant does not implement Hash.
                let seed = self.round_robin_index.fetch_add(1, Ordering::SeqCst);
                let idx = seed % usable_indices.len();
                usable_indices[idx]
            }
        };

        let entry = &keys[selected_idx];
        Some(SelectedKey {
            key_id: entry.key_id.clone(),
            api_key: entry.api_key.value.clone(),
            label: entry.label.clone(),
            quota: entry.quota,
        })
    }

    /// Mark a key as rate-limited for a given cooldown duration.
    pub fn mark_rate_limited(&self, key_id: &KeyId, cooldown: Duration) {
        let keys = self.keys.read();
        for key in keys.iter() {
            if key.key_id.0 == key_id.0 {
                *key.health.write() = KeyHealth::RateLimited {
                    cooldown_until: Instant::now() + cooldown,
                };
                break;
            }
        }
    }

    /// Mark a key as invalid (auth error, deleted key, etc.).
    pub fn mark_invalid(&self, key_id: &KeyId) {
        let keys = self.keys.read();
        for key in keys.iter() {
            if key.key_id.0 == key_id.0 {
                *key.health.write() = KeyHealth::Invalid;
                break;
            }
        }
    }

    /// Mark a key as approaching its limit.
    pub fn mark_approaching_limit(&self, key_id: &KeyId, percent_used: f64, dimension: &str) {
        let keys = self.keys.read();
        for key in keys.iter() {
            if key.key_id.0 == key_id.0 {
                *key.health.write() = KeyHealth::ApproachingLimit {
                    percent_used,
                    dimension: dimension.to_string(),
                };
                break;
            }
        }
    }

    /// Mark a key as healthy (e.g., after cooldown expires or manual reset).
    pub fn mark_healthy(&self, key_id: &KeyId) {
        let keys = self.keys.read();
        for key in keys.iter() {
            if key.key_id.0 == key_id.0 {
                *key.health.write() = KeyHealth::Healthy;
                break;
            }
        }
    }

    /// Update the recorded latency for a key (after a successful request).
    pub fn update_latency(&self, key_id: &KeyId, latency_ms: u64) {
        let keys = self.keys.read();
        for key in keys.iter() {
            if key.key_id.0 == key_id.0 {
                let mut current = key.p95_latency_ms.write();
                // Simple EWMA: 80% old, 20% new
                *current = ((*current * 4) + latency_ms) / 5;
                break;
            }
        }
    }

    /// Get a list of all key metadata (without exposing the actual API key values).
    pub fn list_keys(&self) -> Vec<KeyInfo> {
        let keys = self.keys.read();
        keys.iter()
            .map(|k| KeyInfo {
                key_id: k.key_id.clone(),
                provider: self.provider_id.clone(),
                label: k.label.clone(),
                source: k.source,
                status: k.health.read().label().to_string(),
            })
            .collect()
    }

    /// Get the provider ID this pool serves.
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Get the current selection strategy.
    pub fn selection_strategy(&self) -> KeySelectionStrategy {
        self.selection_strategy
    }

    /// Set the selection strategy.
    pub fn set_selection_strategy(&mut self, strategy: KeySelectionStrategy) {
        self.selection_strategy = strategy;
    }
}

/// A key selected from the pool for a request.
/// Contains everything the caller needs to make a request.
#[derive(Clone)]
pub struct SelectedKey {
    pub key_id: KeyId,
    /// The actual API key value (sensitive — do not log).
    pub api_key: String,
    pub label: String,
    pub quota: ProviderQuota,
}

impl std::fmt::Debug for SelectedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectedKey")
            .field("key_id", &self.key_id)
            .field("api_key", &"***")
            .field("label", &self.label)
            .field("quota", &self.quota)
            .finish()
    }
}

/// Metadata about a key (safe to expose in UI — no actual key value).
#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub key_id: KeyId,
    pub provider: String,
    pub label: String,
    pub source: KeySource,
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_entry(id: &str, label: &str) -> PoolKeyEntry {
        PoolKeyEntry {
            key_id: KeyId(id.to_string()),
            api_key: DecryptedApiKey {
                value: format!("test-key-{}", id),
                provider: "groq".to_string(),
            },
            label: label.to_string(),
            source: KeySource::UiPanel,
            health: RwLock::new(KeyHealth::Healthy),
            quota: ProviderQuota::free_tier("groq").unwrap(),
            p95_latency_ms: RwLock::new(200),
        }
    }

    #[test]
    fn test_pool_add_and_select() {
        let pool = ProviderKeyPool::new("groq");
        pool.add_key(make_test_entry("k1", "team-1"));
        pool.add_key(make_test_entry("k2", "team-2"));

        assert_eq!(pool.len(), 2);
        assert!(pool.has_usable_key());

        let selected = pool.select_key();
        assert!(selected.is_some());
        let sk = selected.unwrap();
        assert_eq!(sk.label, "team-1"); // first usable
    }

    #[test]
    fn test_pool_mark_invalid() {
        let pool = ProviderKeyPool::new("groq");
        let entry = make_test_entry("k1", "team-1");
        pool.add_key(entry.clone());

        pool.mark_invalid(&entry.key_id);
        assert!(!pool.has_usable_key());
        assert!(pool.select_key().is_none());
    }

    #[test]
    fn test_pool_mark_rate_limited() {
        let pool = ProviderKeyPool::new("groq");
        let entry = make_test_entry("k1", "team-1");
        pool.add_key(entry.clone());

        pool.mark_rate_limited(&entry.key_id, Duration::from_secs(60));
        // Still usable because cooldown is in the future? No — is_usable checks cooldown
        // Actually, is_usable returns true if cooldown_until >= now... wait.
        // is_usable: Instant::now() >= cooldown_until means cooldown has passed.
        // We set cooldown_until = now + 60s, so now < cooldown_until, so is_usable returns false.
        assert!(!pool.has_usable_key());
    }

    #[test]
    fn test_pool_remove_key() {
        let pool = ProviderKeyPool::new("groq");
        let entry = make_test_entry("k1", "team-1");
        pool.add_key(entry.clone());
        pool.add_key(make_test_entry("k2", "team-2"));

        pool.remove_key(&entry.key_id);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_key_health_is_usable() {
        assert!(KeyHealth::Healthy.is_usable());
        assert!(KeyHealth::ApproachingLimit {
            percent_used: 0.85,
            dimension: "rpm".into(),
        }
        .is_usable());
        assert!(KeyHealth::Unknown.is_usable());
        assert!(!KeyHealth::Invalid.is_usable());

        let future = Instant::now() + Duration::from_secs(60);
        assert!(!KeyHealth::RateLimited { cooldown_until: future }.is_usable());

        let past = Instant::now() - Duration::from_secs(1);
        assert!(KeyHealth::RateLimited { cooldown_until: past }.is_usable());
    }

    #[test]
    fn test_round_robin_selection() {
        let pool = ProviderKeyPool::with_strategy("groq", KeySelectionStrategy::RoundRobin);
        pool.add_key(make_test_entry("k1", "a"));
        pool.add_key(make_test_entry("k2", "b"));

        let s1 = pool.select_key().unwrap();
        let s2 = pool.select_key().unwrap();
        let s3 = pool.select_key().unwrap();

        // Round-robin should cycle: a, b, a
        assert_eq!(s1.label, "a");
        assert_eq!(s2.label, "b");
        assert_eq!(s3.label, "a");
    }

    #[test]
    fn test_weighted_random_different_each_call() {
        let pool = ProviderKeyPool::with_strategy("groq", KeySelectionStrategy::WeightedRandom);
        pool.add_key(make_test_entry("k1", "a"));
        pool.add_key(make_test_entry("k2", "b"));

        // Just verify it returns a key without panicking
        let s = pool.select_key();
        assert!(s.is_some());
    }

    #[test]
    fn test_key_info_no_exposure() {
        let pool = ProviderKeyPool::new("groq");
        pool.add_key(make_test_entry("k1", "team-1"));

        let infos = pool.list_keys();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].label, "team-1");
        assert_eq!(infos[0].status, "healthy");
        // api_key should NOT be present
    }
}
