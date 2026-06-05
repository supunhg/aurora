use thiserror::Error;

/// Unified error type for the AI subsystem.
#[derive(Error, Debug, Clone)]
pub enum AiError {
    #[error("Provider '{0}' not found")]
    ProviderNotFound(String),

    #[error("No providers registered")]
    NoProviders,

    #[error("All providers failed. Last error: {0}")]
    AllProvidersFailed(String),

    #[error("All API keys exhausted across all providers")]
    AllKeysExhausted,

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Key store error: {0}")]
    KeyStore(String),

    #[error("Rate limit exceeded for {0}")]
    RateLimited(String),

    #[error("Provider error ({0}): {1}")]
    ProviderError(String, String),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Request cancelled")]
    Cancelled,

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<serde_json::Error> for AiError {
    fn from(e: serde_json::Error) -> Self {
        AiError::SerializationError(e.to_string())
    }
}

#[cfg(feature = "cloud-ai")]
impl From<reqwest::Error> for AiError {
    fn from(e: reqwest::Error) -> Self {
        AiError::HttpError(e.to_string())
    }
}

pub type AiResult<T> = Result<T, AiError>;
