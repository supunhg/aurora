use dashmap::DashMap;
use serde_json::Value as JsonValue;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::trace;
use uuid::Uuid;

use crate::connection::{ConnectionError, LspConnection};
use crate::pool::ConnectionPool;

type Result<T> = std::result::Result<T, ConnectionError>;

#[derive(Debug, Clone)]
pub struct LspServerConfig {
    pub language_id: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug)]
struct DebounceState {
    pending_id: Option<Uuid>,
    last_sent: Instant,
}

type DebounceInner = Arc<DashMap<String, DashMap<String, DebounceState>>>;

pub struct LspClient {
    pool: Arc<ConnectionPool>,
    server_configs: Vec<LspServerConfig>,
    debounce: DebounceInner,
    debounce_ms: u64,
}

impl LspClient {
    pub fn new(server_configs: Vec<LspServerConfig>, debounce_ms: u64) -> Self {
        Self {
            pool: Arc::new(ConnectionPool::new()),
            server_configs,
            debounce: Arc::new(DashMap::new()),
            debounce_ms,
        }
    }

    fn config_for(&self, language_id: &str) -> Option<&LspServerConfig> {
        self.server_configs
            .iter()
            .find(|c| c.language_id == language_id)
    }

    pub async fn connection(
        &self,
        language_id: &str,
        workspace_root: &str,
    ) -> Result<Arc<LspConnection>> {
        let config = self.config_for(language_id).ok_or_else(|| {
            ConnectionError::Transport(format!(
                "no server configured for language '{}'",
                language_id
            ))
        })?;
        self.pool
            .get_or_create(language_id, workspace_root, &config.command, &config.args)
            .await
    }

    /// Send a debounced request. The request is queued and only actually sent
    /// after `debounce_ms` milliseconds. If another request with the same
    /// (uri, method) arrives before the timer fires, it replaces the previous
    /// one.
    pub async fn request_debounced(
        &self,
        uri: &str,
        method: &str,
        params: Option<JsonValue>,
        conn: &LspConnection,
    ) -> Option<Result<JsonValue>> {
        let debounce_ms = self.debounce_ms;
        let request_id = Uuid::new_v4();
        let inner = self.debounce.clone();

        // Record the new request ID
        {
            let per_uri = inner.entry(uri.to_string()).or_default();
            per_uri
                .entry(method.to_string())
                .and_modify(|s| {
                    s.pending_id = Some(request_id);
                    s.last_sent = Instant::now();
                })
                .or_insert(DebounceState {
                    pending_id: Some(request_id),
                    last_sent: Instant::now(),
                });
        }

        tokio::time::sleep(Duration::from_millis(debounce_ms)).await;

        // Check if we're still the latest request
        let still_current = inner
            .get(uri)
            .and_then(|per_uri| {
                per_uri
                    .get(method)
                    .map(|s| s.pending_id == Some(request_id))
            })
            .unwrap_or(false);

        if !still_current {
            return None;
        }

        let result = conn.send_request(method, params).await;
        Some(result)
    }

    pub async fn notify(
        &self,
        uri: &str,
        method: &str,
        params: Option<JsonValue>,
        conn: &LspConnection,
    ) -> Result<()> {
        trace!("LSP notify {}: {}", method, uri);
        conn.send_notification(method, params).await
    }

    pub async fn open_document(
        &self,
        uri: &str,
        language_id: &str,
        version: i32,
        text: &str,
        workspace_root: &str,
    ) -> Result<()> {
        let conn = self.connection(language_id, workspace_root).await?;
        conn.did_open(uri, language_id, version, text).await
    }

    pub async fn change_document(
        &self,
        uri: &str,
        language_id: &str,
        version: i32,
        text: &str,
        workspace_root: &str,
    ) -> Result<()> {
        let conn = self.connection(language_id, workspace_root).await?;
        conn.did_change(uri, version, text).await
    }

    pub async fn close_document(
        &self,
        uri: &str,
        language_id: &str,
        workspace_root: &str,
    ) -> Result<()> {
        let conn = self.connection(language_id, workspace_root).await?;
        conn.did_close(uri).await
    }

    pub async fn request_completion(
        &self,
        uri: &str,
        language_id: &str,
        line: u32,
        column: u32,
        workspace_root: &str,
    ) -> Option<Result<JsonValue>> {
        let conn = self.connection(language_id, workspace_root).await.ok()?;
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
        });
        self.request_debounced(uri, "textDocument/completion", Some(params), &conn)
            .await
    }

    pub async fn request_hover(
        &self,
        uri: &str,
        language_id: &str,
        line: u32,
        column: u32,
        workspace_root: &str,
    ) -> Result<JsonValue> {
        let conn = self.connection(language_id, workspace_root).await?;
        conn.hover(uri, line, column).await
    }

    pub async fn request_definition(
        &self,
        uri: &str,
        language_id: &str,
        line: u32,
        column: u32,
        workspace_root: &str,
    ) -> Result<JsonValue> {
        let conn = self.connection(language_id, workspace_root).await?;
        conn.goto_definition(uri, line, column).await
    }

    pub async fn request_references(
        &self,
        uri: &str,
        language_id: &str,
        line: u32,
        column: u32,
        workspace_root: &str,
    ) -> Result<JsonValue> {
        let conn = self.connection(language_id, workspace_root).await?;
        conn.references(uri, line, column).await
    }

    pub async fn shutdown(&self) {
        self.pool.shutdown_all().await;
        self.debounce.clear();
    }
}
