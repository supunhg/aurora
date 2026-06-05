use crate::error::{AiError, AiResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// HTTP client for the FreeLLMAPI sidecar.
///
/// Connects to a local FreeLLMAPI instance (default `http://localhost:3001`)
/// and sends chat completion requests through its unified OpenAI-compatible
/// endpoint with automatic provider fallback.
pub struct FreeLlmClient {
    client: Client,
    base_url: String,
    api_key: String,
}

/// An OpenAI-compatible chat completion request.
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
}

/// A single message in the chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// An OpenAI-compatible chat completion response.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
    pub usage: Option<Usage>,
}

/// A single choice in the chat completion response.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    pub message: ChatResponseMessage,
    pub finish_reason: Option<String>,
}

/// The message content in a response (may include tool calls).
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponseMessage {
    pub role: Option<String>,
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// A tool call from the AI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// A function call within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Token usage statistics.
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// A streaming chunk from SSE.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
}

/// A single choice in a streaming chunk.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    pub delta: Option<StreamDelta>,
    pub finish_reason: Option<String>,
}

/// Delta content in a streaming chunk.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamDelta {
    pub role: Option<String>,
    pub content: Option<String>,
}

impl FreeLlmClient {
    /// Create a new client pointing at the FreeLLMAPI sidecar.
    ///
    /// - `base_url`: The FreeLLMAPI server URL (e.g. `http://localhost:3001`)
    /// - `api_key`: The unified FreeLLMAPI bearer token (starts with `freellmapi-`)
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    /// Create a client with default settings (localhost:3001, dev mode).
    pub fn localhost() -> Self {
        Self::new("http://localhost:3001", "freellmapi-dev")
    }

    /// Discover the unified API key from a running sidecar and create a client.
    /// Falls back to `freellmapi-dev` if the sidecar's key endpoint is unreachable.
    pub async fn discover(base_url: &str) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        let client = Client::new();
        let key_url = format!("{}/api/settings/api-key", base_url);
        let api_key = match client.get(&key_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                body["apiKey"]
                    .as_str()
                    .unwrap_or("freellmapi-dev")
                    .to_string()
            }
            _ => "freellmapi-dev".to_string(),
        };
        Self {
            client,
            base_url,
            api_key,
        }
    }

    /// Register a provider API key with the sidecar.
    pub async fn register_api_key(
        base_url: &str,
        unified_key: &str,
        platform: &str,
        key: &str,
        label: &str,
    ) -> AiResult<()> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let client = Client::new();
        let url = format!("{}/api/keys", base_url);
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", unified_key))
            .json(&serde_json::json!({
                "platform": platform,
                "key": key,
                "label": label,
            }))
            .send()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AiError::ProviderError(
                "sidecar".into(),
                format!("register key HTTP {}: {}", status, body),
            ));
        }
        Ok(())
    }

    /// Get the base URL of this client.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the API key of this client.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Send a chat completion request (non-streaming).
    pub async fn chat_completion(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> AiResult<ChatResponse> {
        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature: Some(0.7),
            max_tokens: Some(2048),
            stream: false,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return if status.as_u16() == 429 {
                Err(AiError::RateLimited(format!("FreeLLMAPI: {}", body)))
            } else {
                Err(AiError::ProviderError(
                    "freellmapi".into(),
                    format!("HTTP {}: {}", status, body),
                ))
            };
        }

        response
            .json::<ChatResponse>()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))
    }

    /// Send a chat completion request with tool definitions.
    /// The AI may respond with tool calls instead of (or in addition to) content.
    pub async fn chat_completion_with_tools(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
    ) -> AiResult<ChatResponse> {
        #[derive(Serialize)]
        struct ToolRequest {
            model: String,
            messages: Vec<ChatMessage>,
            temperature: f32,
            max_tokens: u32,
            stream: bool,
            tools: Vec<serde_json::Value>,
        }

        let request = ToolRequest {
            model: model.to_string(),
            messages,
            temperature: 0.7,
            max_tokens: 4096,
            stream: false,
            tools,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return if status.as_u16() == 429 {
                Err(AiError::RateLimited(format!("FreeLLMAPI: {}", body)))
            } else {
                Err(AiError::ProviderError(
                    "freellmapi".into(),
                    format!("HTTP {}: {}", status, body),
                ))
            };
        }

        response
            .json::<ChatResponse>()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))
    }

    /// Send a streaming chat completion request.
    /// Yields content chunks through the provided channel.
    pub async fn chat_completion_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        tx: mpsc::Sender<String>,
    ) -> AiResult<()> {
        use futures::StreamExt;

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature: Some(0.7),
            max_tokens: Some(2048),
            stream: true,
        };

        let url = format!("{}/v1/chat/completions", self.base_url);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return if status.as_u16() == 429 {
                Err(AiError::RateLimited(format!("FreeLLMAPI: {}", body)))
            } else {
                Err(AiError::ProviderError(
                    "freellmapi".into(),
                    format!("HTTP {}: {}", status, body),
                ))
            };
        }

        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| AiError::HttpError(e.to_string()))?;
            let text = String::from_utf8_lossy(&chunk);

            for line in text.lines() {
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if json_str == "[DONE]" {
                        break;
                    }
                    if let Ok(data) = serde_json::from_str::<StreamChunk>(json_str) {
                        if let Some(choice) = data.choices.first() {
                            if let Some(ref delta) = choice.delta {
                                if let Some(ref content) = delta.content {
                                    if tx.send(content.clone()).await.is_err() {
                                        return Err(AiError::Cancelled);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// List available models from the sidecar.
    pub async fn list_models(&self) -> AiResult<Vec<String>> {
        let url = format!("{}/v1/models", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AiError::ProviderError(
                "freellmapi".into(),
                format!("HTTP {}", response.status()),
            ));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AiError::HttpError(e.to_string()))?;

        let models = body["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    /// Check if the sidecar is reachable.
    pub async fn health_check(&self) -> bool {
        self.client
            .get(format!("{}/v1/models", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = FreeLlmClient::new("http://localhost:3001", "test-key");
        assert_eq!(client.base_url, "http://localhost:3001");
        assert_eq!(client.api_key, "test-key");
    }

    #[test]
    fn test_client_trailing_slash() {
        let client = FreeLlmClient::new("http://localhost:3001/", "test-key");
        assert_eq!(client.base_url, "http://localhost:3001");
    }

    #[test]
    fn test_client_localhost() {
        let client = FreeLlmClient::localhost();
        assert_eq!(client.base_url, "http://localhost:3001");
    }

    #[test]
    fn test_chat_request_serialization() {
        let request = ChatRequest {
            model: "auto".into(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "hello".into(),
            }],
            temperature: Some(0.7),
            max_tokens: Some(100),
            stream: false,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"auto\""));
        assert!(json.contains("\"stream\":false"));
    }
}
