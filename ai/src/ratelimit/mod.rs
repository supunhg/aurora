use dashmap::DashMap;
/// Per-key rate limit tracking.
/// In-memory DashMap counters for hot-path checks + optional SQLite persistence.
/// Tracks RPM (requests per minute), RPD (requests per day), TPM, TPD.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::keystore::quota::ProviderQuota;

#[cfg(feature = "keychain")]
use rusqlite::Connection;

/// Unique key for rate limit tracking.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct RateKey {
    pub provider: String,
    pub model: String,
    pub key_id: String,
}

/// Sliding window counter for a single rate limit dimension.
#[derive(Debug, Clone)]
pub struct SlidingWindow {
    window_duration: Duration,
    max_count: usize,
    timestamps: VecDeque<Instant>,
}

impl SlidingWindow {
    pub fn new(window_duration: Duration, max_count: usize) -> Self {
        Self {
            window_duration,
            max_count,
            timestamps: VecDeque::new(),
        }
    }

    /// Check if a request would exceed the limit.
    /// Uses `count()` which filters expired entries without mutating state.
    pub fn can_accept(&self) -> bool {
        self.count() < self.max_count
    }

    /// Record a request timestamp.
    pub fn record(&mut self) {
        self.evict_old();
        self.timestamps.push_back(Instant::now());
    }

    /// Record a token count (for TPM/TPD tracking).
    ///
    /// TODO: This is O(N) in token count. For large responses, store
    /// `(Instant, usize)` pairs per request and sum counts in `count()`
    /// instead of pushing individual timestamps.
    pub fn record_tokens(&mut self, token_count: usize) {
        self.evict_old();
        for _ in 0..token_count {
            self.timestamps.push_back(Instant::now());
        }
    }

    /// Number of requests in the current window.
    pub fn count(&self) -> usize {
        // Clone to avoid mut borrow issues, then evict
        let cutoff = Instant::now() - self.window_duration;
        self.timestamps.iter().filter(|&&t| t > cutoff).count()
    }

    /// Fraction of limit used (0.0 - 1.0).
    pub fn usage(&self) -> f64 {
        self.count() as f64 / self.max_count as f64
    }

    /// Headroom as a fraction (0.0 - 1.0).
    pub fn headroom(&self) -> f64 {
        1.0 - self.usage()
    }

    /// Check if adding `count` more items would exceed the limit.
    /// Uses `count()` which filters expired entries without mutating state.
    pub fn can_accept_with_count(&self, count: usize) -> bool {
        self.count() + count <= self.max_count
    }

    fn evict_old(&mut self) {
        let cutoff = Instant::now() - self.window_duration;
        while let Some(&front) = self.timestamps.front() {
            if front <= cutoff {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Per-key rate counters.
#[derive(Debug, Clone)]
pub struct RateCounters {
    pub rpm: SlidingWindow,
    pub rpd: SlidingWindow,
    pub tpm: SlidingWindow,
    pub tpd: SlidingWindow,
    pub cooldown_until: Option<Instant>,
}

impl RateCounters {
    pub fn new() -> Self {
        Self::with_quota(&ProviderQuota::default_unknown())
    }

    /// Create rate counters with provider-specific quota limits.
    pub fn with_quota(quota: &ProviderQuota) -> Self {
        Self {
            rpm: SlidingWindow::new(Duration::from_secs(60), quota.rpm as usize),
            rpd: SlidingWindow::new(Duration::from_secs(86400), quota.rpd as usize),
            tpm: SlidingWindow::new(Duration::from_secs(60), quota.tpm as usize),
            tpd: SlidingWindow::new(Duration::from_secs(86400), quota.tpd as usize),
            cooldown_until: None,
        }
    }

    /// Check if a request can proceed (all limits + cooldown).
    pub fn can_request(&self, estimated_tokens: usize) -> bool {
        // Check cooldown first
        if let Some(cooldown) = self.cooldown_until {
            if Instant::now() < cooldown {
                return false;
            }
        }

        self.rpm.can_accept()
            && self.rpd.can_accept()
            && self.tpm.can_accept_with_count(estimated_tokens)
            && self.tpd.can_accept_with_count(estimated_tokens)
    }

    /// Record a request (tokens in/out).
    pub fn record_request(&mut self, tokens_used: usize, success: bool) {
        self.rpm.record();
        self.rpd.record();
        self.tpm.record_tokens(tokens_used);
        self.tpd.record_tokens(tokens_used);

        if !success {
            self.cooldown_until = Some(Instant::now() + Duration::from_secs(10));
        }
    }

    /// Minimum headroom across all dimensions (0.0 - 1.0).
    pub fn min_headroom(&self) -> f64 {
        self.rpm
            .headroom()
            .min(self.rpd.headroom())
            .min(self.tpm.headroom())
            .min(self.tpd.headroom())
    }

    /// Maximum usage percentage across all dimensions (0.0 - 1.0).
    pub fn max_usage(&self) -> f64 {
        1.0 - self.min_headroom()
    }

    /// Check if usage is approaching the limit (>= 80%).
    pub fn is_approaching_limit(&self) -> bool {
        self.max_usage() >= 0.80
    }

    /// Check if usage is critical (>= 95%).
    pub fn is_critical(&self) -> bool {
        self.max_usage() >= 0.95
    }

    /// Get the dimension with the highest usage and its percentage.
    pub fn hottest_dimension(&self) -> (&'static str, f64) {
        let dims = [
            ("rpm", self.rpm.usage()),
            ("rpd", self.rpd.usage()),
            ("tpm", self.tpm.usage()),
            ("tpd", self.tpd.usage()),
        ];
        dims.into_iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(("rpm", 0.0))
    }
}

impl Default for RateCounters {
    fn default() -> Self {
        Self::new()
    }
}

/// The rate limit ledger: manages rate counters for all (provider, model, key) tuples.
pub struct RateLimitLedger {
    counters: DashMap<RateKey, RateCounters>,
    #[allow(dead_code)]
    #[cfg(feature = "keychain")]
    db: Option<std::sync::Mutex<Connection>>,
}

impl RateLimitLedger {
    pub fn new() -> Self {
        Self {
            counters: DashMap::new(),
            #[cfg(feature = "keychain")]
            db: None,
        }
    }

    #[cfg(feature = "keychain")]
    pub fn with_persistence(db: Connection) -> Self {
        Self {
            counters: DashMap::new(),
            db: Some(std::sync::Mutex::new(db)),
        }
    }

    /// Check if a request can proceed for the given key.
    pub fn can_request(&self, key: &RateKey, estimated_tokens: usize) -> bool {
        self.counters
            .get(key)
            .map(|c| c.can_request(estimated_tokens))
            .unwrap_or(true)
    }

    /// Record a successful or failed request.
    pub fn record_request(&self, key: &RateKey, tokens_used: usize, success: bool) {
        let mut entry = self.counters.entry(key.clone()).or_default();
        entry.record_request(tokens_used, success);

        #[cfg(feature = "keychain")]
        if let Some(_db) = self.db.as_ref() {
            self.persist(key, &entry);
        }
    }

    /// Get the minimum headroom for a key (0.0 = fully rate limited, 1.0 = no usage).
    pub fn headroom(&self, key: &RateKey) -> f64 {
        self.counters
            .get(key)
            .map(|c| c.min_headroom())
            .unwrap_or(1.0)
    }

    /// Set a cooldown for a key (e.g., after 429 response).
    pub fn set_cooldown(&self, key: &RateKey, duration: Duration) {
        let mut entry = self.counters.entry(key.clone()).or_default();
        entry.cooldown_until = Some(Instant::now() + duration);
    }

    /// Get provider-level headroom (minimum across all keys for that provider).
    pub fn provider_headroom(&self, provider: &str) -> f64 {
        let mut min_room = 1.0f64;
        for entry in self.counters.iter() {
            if entry.key().provider == provider {
                min_room = min_room.min(entry.min_headroom());
            }
        }
        min_room
    }

    /// Ensure counters exist for a key, creating them with the given quota if missing.
    pub fn ensure_counters(&self, key: &RateKey, quota: &ProviderQuota) {
        self.counters.entry(key.clone()).or_insert_with(|| RateCounters::with_quota(quota));
    }

    /// Get the maximum usage for a specific key (0.0 - 1.0).
    pub fn key_usage(&self, key: &RateKey) -> f64 {
        self.counters
            .get(key)
            .map(|c| c.max_usage())
            .unwrap_or(0.0)
    }

    /// Check if a key is approaching its limit (>= 80%).
    pub fn is_approaching_limit(&self, key: &RateKey) -> bool {
        self.counters
            .get(key)
            .map(|c| c.is_approaching_limit())
            .unwrap_or(false)
    }

    /// Check if a key is critically near its limit (>= 95%).
    pub fn is_critical(&self, key: &RateKey) -> bool {
        self.counters
            .get(key)
            .map(|c| c.is_critical())
            .unwrap_or(false)
    }

    /// Get the hottest dimension and its usage for a key.
    pub fn hottest_dimension(&self, key: &RateKey) -> Option<(&'static str, f64)> {
        self.counters.get(key).map(|c| c.hottest_dimension())
    }

    /// Number of tracked keys.
    pub fn tracked_keys(&self) -> usize {
        self.counters.len()
    }

    #[cfg(feature = "keychain")]
    fn persist(&self, _key: &RateKey, _counters: &RateCounters) {
        // TODO: Persist rate counters to SQLite for crash recovery
    }
}

impl Default for RateLimitLedger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliding_window_accepts_initial() {
        let window = SlidingWindow::new(Duration::from_secs(60), 5);
        assert!(window.can_accept());
    }

    #[test]
    fn test_sliding_window_rejects_at_limit() {
        let mut window = SlidingWindow::new(Duration::from_secs(60), 3);
        assert!(window.can_accept());
        window.record();
        assert!(window.can_accept());
        window.record();
        assert!(window.can_accept());
        window.record();
        assert!(!window.can_accept());
    }

    #[test]
    fn test_rate_key_uniqueness() {
        let key1 = RateKey {
            provider: "groq".into(),
            model: "llama3".into(),
            key_id: "key-1".into(),
        };
        let key2 = RateKey {
            provider: "groq".into(),
            model: "llama3".into(),
            key_id: "key-2".into(),
        };
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_ledger_headroom() {
        let ledger = RateLimitLedger::new();
        let key = RateKey {
            provider: "test".into(),
            model: "test".into(),
            key_id: "test".into(),
        };
        assert!((ledger.headroom(&key) - 1.0).abs() < f64::EPSILON);

        // After filling RPM, headroom should drop
        let mut counters = RateCounters::new();
        for _ in 0..60 {
            counters.record_request(100, true);
        }
        ledger.counters.insert(key.clone(), counters);
        assert!(ledger.headroom(&key) < 0.1);
    }
}
