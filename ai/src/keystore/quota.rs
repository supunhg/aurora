/// Provider-specific quota definitions for rate limit tracking.
///
/// Each provider has documented free-tier limits for RPM (requests per minute),
/// RPD (requests per day), TPM (tokens per minute), and TPD (tokens per day).
/// These defaults are based on provider documentation as of June 2026.
/// Users can override per-key via the Key Management UI or config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderQuota {
    /// Requests per minute.
    pub rpm: u32,
    /// Requests per day.
    pub rpd: u32,
    /// Tokens per minute.
    pub tpm: u32,
    /// Tokens per day.
    pub tpd: u32,
    /// Burst allowance (some providers allow brief spikes above the sustained rate).
    pub tpm_burst: u32,
}

impl ProviderQuota {
    /// Create a quota with all dimensions set to the same value (useful for unlimited/local).
    pub fn unlimited() -> Self {
        Self {
            rpm: u32::MAX,
            rpd: u32::MAX,
            tpm: u32::MAX,
            tpd: u32::MAX,
            tpm_burst: u32::MAX,
        }
    }

    /// Get documented free-tier quotas for known providers.
    /// Returns `None` for unknown providers — caller should use a conservative default.
    pub fn free_tier(provider_id: &str) -> Option<Self> {
        match provider_id {
            "groq" => Some(Self {
                rpm: 20,
                rpd: 1_440,
                tpm: 25_000,
                tpd: 500_000,
                tpm_burst: 30_000,
            }),
            "gemini" | "google" => Some(Self {
                rpm: 60,
                rpd: 1_500,
                tpm: 1_000_000,
                tpd: 1_000_000,
                tpm_burst: 1_500_000,
            }),
            "cerebras" => Some(Self {
                rpm: 30,
                rpd: 1_000,
                tpm: 100_000,
                tpd: 1_000_000,
                tpm_burst: 150_000,
            }),
            "together" => Some(Self {
                rpm: 60,
                rpd: 1_440,
                tpm: 100_000,
                tpd: 200_000,
                tpm_burst: 150_000,
            }),
            "openai" => Some(Self {
                rpm: 3,
                rpd: 200,
                tpm: 150_000,
                tpd: 150_000,
                tpm_burst: 200_000,
            }),
            "anthropic" => Some(Self {
                rpm: 5,
                rpd: 100,
                tpm: 25_000,
                tpd: 100_000,
                tpm_burst: 30_000,
            }),
            "ollama" | "local" => Some(Self::unlimited()),
            _ => None,
        }
    }

    /// Get a conservative default quota for unknown providers.
    pub fn default_unknown() -> Self {
        Self {
            rpm: 10,
            rpd: 500,
            tpm: 50_000,
            tpd: 500_000,
            tpm_burst: 60_000,
        }
    }

    /// Percentage used for a given dimension (0.0–1.0).
    pub fn percent_used(&self, used: u32, dimension: &str) -> f64 {
        let limit = match dimension {
            "rpm" => self.rpm,
            "rpd" => self.rpd,
            "tpm" => self.tpm,
            "tpd" => self.tpd,
            _ => return 0.0,
        };
        if limit == 0 || limit == u32::MAX {
            return 0.0;
        }
        (used as f64) / (limit as f64)
    }
}

impl Default for ProviderQuota {
    fn default() -> Self {
        Self::default_unknown()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_groq_free_tier() {
        let q = ProviderQuota::free_tier("groq").unwrap();
        assert_eq!(q.rpm, 20);
        assert_eq!(q.tpm, 25_000);
    }

    #[test]
    fn test_gemini_free_tier() {
        let q = ProviderQuota::free_tier("gemini").unwrap();
        assert_eq!(q.rpm, 60);
        assert_eq!(q.tpm, 1_000_000);
    }

    #[test]
    fn test_ollama_unlimited() {
        let q = ProviderQuota::free_tier("ollama").unwrap();
        assert_eq!(q.rpm, u32::MAX);
    }

    #[test]
    fn test_unknown_provider() {
        assert!(ProviderQuota::free_tier("unknown_provider").is_none());
    }

    #[test]
    fn test_percent_used() {
        let q = ProviderQuota::free_tier("groq").unwrap();
        assert!((q.percent_used(10, "rpm") - 0.5).abs() < 0.01);
        assert!((q.percent_used(16, "rpm") - 0.8).abs() < 0.01);
        assert!((q.percent_used(20, "rpm") - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_default_unknown() {
        let q = ProviderQuota::default_unknown();
        assert_eq!(q.rpm, 10);
        assert_eq!(q.rpd, 500);
    }
}
