use std::sync::Arc;

use crate::client::{LspClient, LspServerConfig};
use editor::{EditorEvent, EventCollector};
use tracing::{debug, warn};

/// Bridges editor events to the LSP client.
///
/// Consumes `EditorEvent`s from the editor's event collector and translates
/// them into LSP protocol messages (didOpen, didChange, didClose, completion, etc.)
pub struct LspBridge {
    client: Arc<LspClient>,
    workspace_root: String,
}

impl LspBridge {
    /// Create a new bridge connecting editor events to an LSP client.
    pub fn new(client: Arc<LspClient>, workspace_root: &str) -> Self {
        Self {
            client,
            workspace_root: workspace_root.to_string(),
        }
    }

    /// Create a bridge with common LSP server configurations.
    pub fn with_defaults(workspace_root: &str) -> Self {
        let configs = vec![
            LspServerConfig {
                language_id: "rust".into(),
                command: "rust-analyzer".into(),
                args: vec![],
            },
            LspServerConfig {
                language_id: "typescript".into(),
                command: "typescript-language-server".into(),
                args: vec!["--stdio".into()],
            },
            LspServerConfig {
                language_id: "javascript".into(),
                command: "typescript-language-server".into(),
                args: vec!["--stdio".into()],
            },
            LspServerConfig {
                language_id: "python".into(),
                command: "pylsp".into(),
                args: vec![],
            },
            LspServerConfig {
                language_id: "go".into(),
                command: "gopls".into(),
                args: vec![],
            },
        ];

        let client = Arc::new(LspClient::new(configs, 150));
        Self::new(client, workspace_root)
    }

    /// Process all pending events from the collector.
    pub async fn process_events(&self, collector: &mut EventCollector) {
        let events = collector.drain();
        for event in events {
            if let Err(e) = self.handle_event(event).await {
                warn!("[lsp-bridge] Error handling event: {}", e);
            }
        }
    }

    /// Handle a single editor event.
    async fn handle_event(&self, event: EditorEvent) -> Result<(), String> {
        match event {
            EditorEvent::FileOpened {
                uri,
                language_id,
                text,
            } => {
                debug!("[lsp-bridge] didOpen {} ({})", uri, language_id);
                let conn = self
                    .client
                    .connection(&language_id, &self.workspace_root)
                    .await
                    .map_err(|e| e.to_string())?;
                conn.did_open(&uri, &language_id, 1, &text)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            EditorEvent::BufferChanged { uri, version, text } => {
                debug!("[lsp-bridge] didChange {} v{}", uri, version);
                // Try to find the language from the URI
                if let Some(language_id) = language_from_uri(&uri) {
                    if let Ok(conn) = self
                        .client
                        .connection(&language_id, &self.workspace_root)
                        .await
                    {
                        let _ = conn.did_change(&uri, version, &text).await;
                    }
                }
            }
            EditorEvent::FileClosed { uri } => {
                debug!("[lsp-bridge] didClose {}", uri);
                if let Some(language_id) = language_from_uri(&uri) {
                    if let Ok(conn) = self
                        .client
                        .connection(&language_id, &self.workspace_root)
                        .await
                    {
                        let _ = conn.did_close(&uri).await;
                    }
                }
            }
            EditorEvent::CursorMoved { uri, line, column } => {
                // Cursor movement is handled separately for completions/hover
                debug!("[lsp-bridge] cursor moved {}:{}:{}", uri, line, column);
            }
            EditorEvent::FileFocused { uri } => {
                debug!("[lsp-bridge] file focused {}", uri);
            }
        }
        Ok(())
    }

    /// Request completions at the given position.
    pub async fn request_completions(
        &self,
        uri: &str,
        line: u32,
        column: u32,
    ) -> Option<serde_json::Value> {
        let language_id = language_from_uri(uri)?;
        let _conn = self
            .client
            .connection(&language_id, &self.workspace_root)
            .await
            .ok()?;
        self.client
            .request_completion(uri, &language_id, line, column, &self.workspace_root)
            .await?
            .ok()
    }

    /// Request hover information at the given position.
    pub async fn request_hover(
        &self,
        uri: &str,
        line: u32,
        column: u32,
    ) -> Option<serde_json::Value> {
        let language_id = language_from_uri(uri)?;
        let _conn = self
            .client
            .connection(&language_id, &self.workspace_root)
            .await
            .ok()?;
        self.client
            .request_hover(uri, &language_id, line, column, &self.workspace_root)
            .await
            .ok()
    }

    /// Request go-to-definition at the given position.
    pub async fn request_definition(
        &self,
        uri: &str,
        line: u32,
        column: u32,
    ) -> Option<serde_json::Value> {
        let language_id = language_from_uri(uri)?;
        let _conn = self
            .client
            .connection(&language_id, &self.workspace_root)
            .await
            .ok()?;
        self.client
            .request_definition(uri, &language_id, line, column, &self.workspace_root)
            .await
            .ok()
    }

    /// Request references at the given position.
    pub async fn request_references(
        &self,
        uri: &str,
        line: u32,
        column: u32,
    ) -> Option<serde_json::Value> {
        let language_id = language_from_uri(uri)?;
        let _conn = self
            .client
            .connection(&language_id, &self.workspace_root)
            .await
            .ok()?;
        self.client
            .request_references(uri, &language_id, line, column, &self.workspace_root)
            .await
            .ok()
    }

    /// Shutdown all LSP connections.
    pub async fn shutdown(&self) {
        self.client.shutdown().await;
    }
}

/// Extract language ID from a file:// URI.
fn language_from_uri(uri: &str) -> Option<String> {
    let path_str = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("file:///"))?;
    let path = std::path::Path::new(path_str);
    Some(editor::detect_language(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_from_uri() {
        assert_eq!(
            language_from_uri("file:///home/user/main.rs").as_deref(),
            Some("rust")
        );
        assert_eq!(
            language_from_uri("file:///src/app.ts").as_deref(),
            Some("typescript")
        );
        assert_eq!(
            language_from_uri("file:///test.py").as_deref(),
            Some("python")
        );
    }

    #[test]
    fn test_language_detection() {
        use editor::detect_language;
        assert_eq!(detect_language(std::path::Path::new("main.rs")), "rust");
        assert_eq!(
            detect_language(std::path::Path::new("app.ts")),
            "typescript"
        );
        assert_eq!(detect_language(std::path::Path::new("index.html")), "html");
        assert_eq!(
            detect_language(std::path::Path::new("unknown")),
            "plaintext"
        );
    }
}
