use dashmap::DashMap;
use parking_lot::RwLock;
/// Background health monitor for AI providers and API keys.
/// Probes each key periodically and updates its health state.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Health state of a provider key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HealthState {
    /// Key is working normally.
    Healthy,
    /// Key is currently rate-limited (waiting for cooldown).
    RateLimited,
    /// Key is invalid (auth error, deleted, etc.).
    Invalid,
    /// Last health check returned an error (network, timeout, etc.).
    Error,
    /// No health check has been performed yet.
    Unknown,
}

impl HealthState {
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Healthy | Self::Unknown)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::RateLimited => "rate_limited",
            Self::Invalid => "invalid",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }
}

/// A health check entry for a single key.
#[derive(Debug, Clone)]
pub struct HealthEntry {
    pub provider: String,
    pub model: String,
    pub key_id: String,
    pub state: HealthState,
    pub last_checked: Option<std::time::Instant>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
}

impl HealthEntry {
    pub fn new(provider: &str, model: &str, key_id: &str) -> Self {
        Self {
            provider: provider.to_string(),
            model: model.to_string(),
            key_id: key_id.to_string(),
            state: HealthState::Unknown,
            last_checked: None,
            last_error: None,
            consecutive_failures: 0,
        }
    }
}

/// A health check probe function.
/// Returns Ok(()) if the provider/key is healthy, Err with details otherwise.
pub type HealthProbe = Box<dyn Fn(&str, &str) -> Result<(), String> + Send + Sync>;

/// Background health monitor that periodically checks provider key health.
pub struct HealthMonitor {
    entries: DashMap<String, HealthEntry>,
    probes: Arc<RwLock<HashMap<String, Vec<HealthProbe>>>>,
    interval: Duration,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl HealthMonitor {
    /// Create a new health monitor with the given check interval.
    pub fn new(interval_secs: u64) -> Self {
        Self {
            entries: DashMap::new(),
            probes: Arc::new(RwLock::new(HashMap::new())),
            interval: Duration::from_secs(interval_secs),
            shutdown_tx: None,
        }
    }

    /// Register a health probe for a provider.
    /// The probe receives (model, key_value) and returns Ok/Err.
    pub fn register_probe(&self, provider: &str, probe: HealthProbe) {
        self.probes
            .write()
            .entry(provider.to_string())
            .or_default()
            .push(probe);
    }

    /// Register a key for health monitoring.
    pub fn register_key(&self, provider: &str, model: &str, key_id: &str) {
        self.entries.insert(
            key_id.to_string(),
            HealthEntry::new(provider, model, key_id),
        );
    }

    /// Remove a key from health monitoring.
    pub fn unregister_key(&self, key_id: &str) {
        self.entries.remove(key_id);
    }

    /// Get the health state of a specific key.
    pub fn key_health(&self, key_id: &str) -> HealthState {
        self.entries
            .get(key_id)
            .map(|e| e.state)
            .unwrap_or(HealthState::Unknown)
    }

    /// Get health states for all keys of a provider.
    pub fn provider_health(&self, provider: &str) -> Vec<HealthState> {
        self.entries
            .iter()
            .filter(|e| e.provider == provider)
            .map(|e| e.state)
            .collect()
    }

    /// Check if any key for a provider is usable.
    pub fn provider_has_usable_key(&self, provider: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.provider == provider && e.state.is_usable())
    }

    /// Get all entries (for UI display).
    pub fn all_entries(&self) -> Vec<HealthEntry> {
        self.entries.iter().map(|e| e.clone()).collect()
    }

    /// Run a single health check cycle for all registered keys.
    pub async fn check_all(&self) {
        let probes = self.probes.read();
        let keys: Vec<(String, String, String)> = self
            .entries
            .iter()
            .map(|e| (e.provider.clone(), e.model.clone(), e.key_id.clone()))
            .collect();

        for (provider, model, key_id) in &keys {
            if let Some(provider_probes) = probes.get(provider) {
                for probe in provider_probes {
                    match probe(model, key_id) {
                        Ok(()) => {
                            if let Some(mut entry) = self.entries.get_mut(key_id.as_str()) {
                                entry.state = HealthState::Healthy;
                                entry.last_checked = Some(std::time::Instant::now());
                                entry.last_error = None;
                                entry.consecutive_failures = 0;
                            }
                        }
                        Err(err) => {
                            if let Some(mut entry) = self.entries.get_mut(key_id.as_str()) {
                                entry.consecutive_failures += 1;
                                entry.last_checked = Some(std::time::Instant::now());
                                entry.last_error = Some(err.clone());

                                // Classify the error
                                let err_lower = err.to_lowercase();
                                if err_lower.contains("401")
                                    || err_lower.contains("unauthorized")
                                    || err_lower.contains("invalid")
                                    || err_lower.contains("auth")
                                {
                                    entry.state = HealthState::Invalid;
                                    error!("Key {} ({}) is INVALID: {}", key_id, provider, err);
                                } else if err_lower.contains("429")
                                    || err_lower.contains("rate limit")
                                {
                                    entry.state = HealthState::RateLimited;
                                    warn!("Key {} ({}) is RATE LIMITED: {}", key_id, provider, err);
                                } else if entry.consecutive_failures >= 3 {
                                    entry.state = HealthState::Error;
                                    error!(
                                        "Key {} ({}) failed {} times: {}",
                                        key_id, provider, entry.consecutive_failures, err
                                    );
                                } else {
                                    // Transient error, keep as Unknown/Healthy for now
                                    info!("Key {} ({}) transient error: {}", key_id, provider, err);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Start the background health check loop.
    /// Runs checks every `self.interval` until shutdown signal is received.
    pub async fn start(&mut self) {
        let (tx, mut rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(tx);

        let interval = self.interval;
        info!("Health monitor started, checking every {:?}", interval);

        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {
                    self.check_all().await;
                }
                _ = rx.recv() => {
                    info!("Health monitor shutting down");
                    break;
                }
            }
        }
    }

    /// Signal the health monitor to shut down.
    pub fn shutdown(&self) {
        if let Some(tx) = &self.shutdown_tx {
            let _ = tx.try_send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_state_usable() {
        assert!(HealthState::Healthy.is_usable());
        assert!(HealthState::Unknown.is_usable());
        assert!(!HealthState::RateLimited.is_usable());
        assert!(!HealthState::Invalid.is_usable());
        assert!(!HealthState::Error.is_usable());
    }

    #[test]
    fn test_register_and_check() {
        let monitor = HealthMonitor::new(3600);
        monitor.register_key("test", "test-model", "test-key");

        let entry = monitor.entries.get("test-key").unwrap();
        assert_eq!(entry.state, HealthState::Unknown);
        assert_eq!(entry.provider, "test");
    }
}
