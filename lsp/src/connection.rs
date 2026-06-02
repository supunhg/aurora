use dashmap::DashMap;
use lsp_types::{
    ClientCapabilities, GeneralClientCapabilities, InitializeParams, PositionEncodingKind,
    ServerCapabilities, TextDocumentClientCapabilities,
};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, trace, warn};

use crate::transport::read_message;

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("Transport error: {0}")]
    Transport(String),
    #[error("Server not initialized")]
    NotInitialized,
    #[error("Request {0} timed out")]
    Timeout(u64),
    #[error("Server error {code}: {message}", code = .code, message = .message)]
    ServerError { code: i64, message: String },
    #[error("Server exited")]
    ServerExited,
}

type Result<T> = std::result::Result<T, ConnectionError>;

/// Manages a single LSP server process.
pub struct LspConnection {
    child: Mutex<Option<Child>>,
    stdin: Mutex<tokio::process::ChildStdin>,
    server_caps: tokio::sync::RwLock<Option<ServerCapabilities>>,
    next_id: std::sync::atomic::AtomicU64,
    pending: Arc<DashMap<u64, tokio::sync::oneshot::Sender<Result<JsonValue>>>>,
    initialized: std::sync::atomic::AtomicBool,
}

impl LspConnection {
    pub async fn spawn(command: &str, args: &[String]) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ConnectionError::Transport(e.to_string()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ConnectionError::Transport("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ConnectionError::Transport("no stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ConnectionError::Transport("no stderr".into()))?;

        // Log stderr
        tokio::spawn(async move {
            let mut err_reader = BufReader::new(stderr);
            let mut buf = String::new();
            loop {
                buf.clear();
                match err_reader.read_line(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => warn!("[LSP stderr] {}", buf.trim_end()),
                }
            }
        });

        let pending: Arc<DashMap<u64, tokio::sync::oneshot::Sender<Result<JsonValue>>>> =
            Arc::new(DashMap::new());
        let bg_pending = pending.clone();

        // Background reader for stdout
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader).await {
                    Ok(Some(raw)) => {
                        let parsed: serde_json::Value = match serde_json::from_str(&raw) {
                            Ok(v) => v,
                            Err(e) => {
                                warn!("Failed to parse LSP message: {}", e);
                                continue;
                            }
                        };

                        if let Some(id_val) = parsed.get("id") {
                            if let Some(id) = id_val.as_u64() {
                                if let Some((_, tx)) = bg_pending.remove(&id) {
                                    if let Some(error) = parsed.get("error") {
                                        let code = error["code"].as_i64().unwrap_or(-1);
                                        let msg = error["message"]
                                            .as_str()
                                            .unwrap_or("unknown")
                                            .to_string();
                                        let _ = tx.send(Err(ConnectionError::ServerError {
                                            code,
                                            message: msg,
                                        }));
                                    } else {
                                        let result = parsed
                                            .get("result")
                                            .cloned()
                                            .unwrap_or(JsonValue::Null);
                                        let _ = tx.send(Ok(result));
                                    }
                                }
                                continue;
                            }
                        }

                        if let Some(method) = parsed.get("method").and_then(|m| m.as_str()) {
                            match method {
                                "textDocument/publishDiagnostics" => debug!("Diagnostics received"),
                                "$/progress" => {}
                                "window/showMessage" => {
                                    if let Some(msg) = parsed
                                        .get("params")
                                        .and_then(|p| p["message"].as_str().map(String::from))
                                    {
                                        info!("[LSP] {}", msg);
                                    }
                                }
                                "window/logMessage" => {
                                    if let Some(msg) = parsed
                                        .get("params")
                                        .and_then(|p| p["message"].as_str().map(String::from))
                                    {
                                        debug!("[LSP log] {}", msg);
                                    }
                                }
                                other => trace!("Unhandled notification: {}", other),
                            }
                        }
                    }
                    Ok(None) => {
                        debug!("LSP stdout closed");
                        break;
                    }
                    Err(e) => {
                        warn!("LSP read error: {}", e);
                        break;
                    }
                }
            }
            // Transfer all pending -> cancelled
            let keys: Vec<u64> = bg_pending.iter().map(|e| *e.key()).collect();
            for id in keys {
                if let Some((_, tx)) = bg_pending.remove(&id) {
                    let _ = tx.send(Err(ConnectionError::ServerExited));
                }
            }
        });

        Ok(Self {
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(stdin),
            server_caps: tokio::sync::RwLock::new(None),
            next_id: std::sync::atomic::AtomicU64::new(1),
            pending,
            initialized: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub async fn send_request(&self, method: &str, params: Option<JsonValue>) -> Result<JsonValue> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.insert(id, tx);

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let body_str =
            serde_json::to_string(&body).map_err(|e| ConnectionError::Transport(e.to_string()))?;
        self.write_raw(body_str).await?;

        tokio::select! {
            result = rx => {
                self.pending.remove(&id);
                match result {
                    Ok(Ok(val)) => Ok(val),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err(ConnectionError::Timeout(id)),
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                self.pending.remove(&id);
                Err(ConnectionError::Timeout(id))
            }
        }
    }

    pub async fn send_notification(&self, method: &str, params: Option<JsonValue>) -> Result<()> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let body_str =
            serde_json::to_string(&body).map_err(|e| ConnectionError::Transport(e.to_string()))?;
        self.write_raw(body_str).await
    }

    async fn write_raw(&self, body: String) -> Result<()> {
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(header.as_bytes())
            .await
            .map_err(|e| ConnectionError::Transport(e.to_string()))?;
        stdin
            .write_all(body.as_bytes())
            .await
            .map_err(|e| ConnectionError::Transport(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| ConnectionError::Transport(e.to_string()))?;
        trace!("LSP sent: {} bytes", body.len());
        Ok(())
    }

    pub async fn initialize(&self, workspace_root: Option<&str>) -> Result<ServerCapabilities> {
        let root_uri = workspace_root.and_then(|r| url::Url::parse(&format!("file://{}", r)).ok());

        #[allow(deprecated)]
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            client_info: Some(lsp_types::ClientInfo {
                name: "aurora".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    synchronization: Some(lsp_types::TextDocumentSyncClientCapabilities {
                        dynamic_registration: Some(true),
                        ..Default::default()
                    }),
                    completion: Some(lsp_types::CompletionClientCapabilities {
                        dynamic_registration: Some(true),
                        completion_item: Some(lsp_types::CompletionItemCapability {
                            snippet_support: Some(true),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    hover: Some(lsp_types::HoverClientCapabilities {
                        dynamic_registration: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                general: Some(GeneralClientCapabilities {
                    position_encodings: Some(vec![PositionEncodingKind::UTF16]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            root_uri,
            workspace_folders: None,
            ..Default::default()
        };

        let req =
            serde_json::to_value(&params).map_err(|e| ConnectionError::Transport(e.to_string()))?;
        let resp = self.send_request("initialize", Some(req)).await?;

        let caps: ServerCapabilities =
            serde_json::from_value(resp).map_err(|e| ConnectionError::Transport(e.to_string()))?;

        *self.server_caps.write().await = Some(caps.clone());
        self.send_notification("initialized", None).await?;
        self.initialized
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(caps)
    }

    pub async fn capabilities(&self) -> Option<ServerCapabilities> {
        self.server_caps.read().await.clone()
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub async fn shutdown(&self) -> Result<()> {
        let _ = self.send_request("shutdown", None).await;
        self.send_notification("exit", None).await.ok();
        let mut child_guard = self.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            child.kill().await.ok();
            child.wait().await.ok();
        }
        Ok(())
    }

    pub async fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i32,
        text: &str,
    ) -> Result<()> {
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": version,
                "text": text,
            }
        });
        self.send_notification("textDocument/didOpen", Some(params))
            .await
    }

    pub async fn did_change(&self, uri: &str, version: i32, text: &str) -> Result<()> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": text }],
        });
        self.send_notification("textDocument/didChange", Some(params))
            .await
    }

    pub async fn did_close(&self, uri: &str) -> Result<()> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });
        self.send_notification("textDocument/didClose", Some(params))
            .await
    }

    pub async fn completion(&self, uri: &str, line: u32, column: u32) -> Result<JsonValue> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
        });
        self.send_request("textDocument/completion", Some(params))
            .await
    }

    pub async fn hover(&self, uri: &str, line: u32, column: u32) -> Result<JsonValue> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
        });
        self.send_request("textDocument/hover", Some(params)).await
    }

    pub async fn goto_definition(&self, uri: &str, line: u32, column: u32) -> Result<JsonValue> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
        });
        self.send_request("textDocument/definition", Some(params))
            .await
    }

    pub async fn references(&self, uri: &str, line: u32, column: u32) -> Result<JsonValue> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": column },
        });
        self.send_request("textDocument/references", Some(params))
            .await
    }
}

impl Drop for LspConnection {
    fn drop(&mut self) {
        // Child handle is behind Mutex, so we can't easily kill in Drop.
        // shutdown() should be called explicitly.
    }
}
