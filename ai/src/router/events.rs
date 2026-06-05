/// Events emitted by the key rotation system for UI and logging consumption.
#[derive(Debug, Clone)]
pub enum KeyRotationEvent {
    /// A key was rotated because the previous one hit a limit.
    KeyRotated {
        provider: String,
        from_key_id: String,
        from_key_label: String,
        to_key_id: String,
        to_key_label: String,
        reason: RotationReason,
    },
    /// A key is approaching its limit (>= 80%).
    KeyApproachingLimit {
        provider: String,
        key_id: String,
        key_label: String,
        percent_used: f64,
        dimension: String,
    },
    /// A key crossed into critical territory (>= 95%).
    KeyCritical {
        provider: String,
        key_id: String,
        key_label: String,
        percent_used: f64,
        dimension: String,
    },
    /// All keys for a provider are exhausted.
    ProviderExhausted {
        provider: String,
    },
    /// All keys across ALL providers are exhausted.
    AllKeysExhausted,
    /// A request succeeded — usage updated.
    UsageUpdated {
        provider: String,
        key_id: String,
        key_label: String,
        percent_used: f64,
        dimension: String,
    },
    /// A key was added to a pool.
    KeyAdded {
        provider: String,
        key_id: String,
        key_label: String,
        source: String,
    },
    /// A key was removed from a pool.
    KeyRemoved {
        provider: String,
        key_id: String,
    },
    /// A key was marked invalid (auth error, etc.).
    KeyInvalidated {
        provider: String,
        key_id: String,
        key_label: String,
        error: String,
    },
    /// A key's cooldown has expired and it is usable again.
    KeyRecovered {
        provider: String,
        key_id: String,
        key_label: String,
    },
}

/// Why a key was rotated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationReason {
    /// The previous key returned a 429 / rate limit error.
    RateLimited,
    /// The previous key was invalid (401/403).
    InvalidKey,
    /// The previous key failed with a transient error (timeout, 5xx, network, etc.).
    TransientError,
}

impl RotationReason {
    pub fn label(&self) -> &'static str {
        match self {
            RotationReason::RateLimited => "rate_limited",
            RotationReason::InvalidKey => "invalid_key",
            RotationReason::TransientError => "transient_error",
        }
    }
}

impl std::fmt::Display for RotationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}
