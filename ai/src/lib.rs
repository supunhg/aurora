pub mod agent;
pub mod context;
pub mod error;
pub mod freellm;
pub mod health;
pub mod keystore;
pub mod providers;
pub mod ratelimit;
pub mod router;
pub mod sidecar;

pub use agent::{AgentLoop, AgentResult, AgentStatus};
pub use error::{AiError, AiResult};
pub use freellm::FreeLlmClient;
pub use keystore::{DecryptedApiKey, EphemeralKeyStore, KeyId};
pub use providers::ProviderAdapter;
pub use ratelimit::{RateCounters, RateKey, RateLimitLedger};
pub use router::AIRouter;
pub use sidecar::SidecarManager;

pub use providers::{GroqProvider, OllamaProvider, OpenAIProvider};

/// Re-export mock providers for testing.
pub mod mock {
    pub use crate::providers::{LocalProvider, MockCloudProvider};
}
