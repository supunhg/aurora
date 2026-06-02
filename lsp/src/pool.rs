use dashmap::DashMap;
use std::sync::Arc;
use tracing::debug;

use crate::connection::{ConnectionError, LspConnection};

type Result<T> = std::result::Result<T, ConnectionError>;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ConnectionKey {
    pub language_id: String,
    pub workspace_root: String,
}

pub struct ConnectionPool {
    connections: DashMap<ConnectionKey, Arc<LspConnection>>,
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionPool {
    pub fn new() -> Self {
        Self {
            connections: DashMap::new(),
        }
    }

    pub async fn get_or_create(
        &self,
        language_id: &str,
        workspace_root: &str,
        server_command: &str,
        server_args: &[String],
    ) -> Result<Arc<LspConnection>> {
        let key = ConnectionKey {
            language_id: language_id.to_string(),
            workspace_root: workspace_root.to_string(),
        };

        if let Some(entry) = self.connections.get(&key) {
            if entry.is_initialized() {
                return Ok(entry.clone());
            }
            drop(entry);
            self.connections.remove(&key);
        }

        debug!(
            "Spawning LSP server: {} for language={}",
            server_command, language_id
        );
        let conn = LspConnection::spawn(server_command, server_args).await?;
        let caps = conn.initialize(Some(workspace_root)).await?;
        debug!(
            "LSP server initialized for {}: text_document_sync={:?}",
            language_id, caps.text_document_sync
        );

        let conn = Arc::new(conn);
        self.connections.insert(key, conn.clone());
        Ok(conn)
    }

    pub fn get(&self, language_id: &str, workspace_root: &str) -> Option<Arc<LspConnection>> {
        let key = ConnectionKey {
            language_id: language_id.to_string(),
            workspace_root: workspace_root.to_string(),
        };
        self.connections.get(&key).map(|e| e.clone())
    }

    pub async fn remove(&self, language_id: &str, workspace_root: &str) {
        let key = ConnectionKey {
            language_id: language_id.to_string(),
            workspace_root: workspace_root.to_string(),
        };
        if let Some((_, conn)) = self.connections.remove(&key) {
            conn.shutdown().await.ok();
        }
    }

    pub async fn shutdown_all(&self) {
        for entry in self.connections.iter() {
            entry.shutdown().await.ok();
        }
        self.connections.clear();
    }

    pub fn len(&self) -> usize {
        self.connections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}
