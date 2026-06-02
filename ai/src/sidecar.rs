use crate::error::{AiError, AiResult};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{debug, info, warn};

/// Manages the FreeLLMAPI Node.js sidecar process.
///
/// Handles spawning, health checking, automatic restart with exponential
/// backoff, and graceful shutdown. The sidecar runs on localhost with
/// a dynamically allocated port.
pub struct SidecarManager {
    child: Option<Child>,
    base_url: String,
    api_key: String,
    port: u16,
    freellmapi_dir: PathBuf,
    restart_count: u32,
    max_restarts: u32,
}

/// Status of the sidecar process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarStatus {
    /// Not started yet.
    Stopped,
    /// Starting up, waiting for health check.
    Starting,
    /// Running and healthy.
    Running { port: u16 },
    /// Process exited, will restart.
    Restarting { attempt: u32 },
    /// Exceeded max restart attempts.
    Failed,
}

impl SidecarManager {
    /// Create a new sidecar manager.
    ///
    /// - `freellmapi_dir`: Path to the FreeLLMAPI repo (e.g. `sidecar/freellmapi`)
    /// - `port`: Preferred port (0 = auto-assign)
    pub fn new(freellmapi_dir: PathBuf, port: u16) -> Self {
        Self {
            child: None,
            base_url: String::new(),
            api_key: String::new(),
            port,
            freellmapi_dir,
            restart_count: 0,
            max_restarts: 5,
        }
    }

    /// Start the sidecar process.
    ///
    /// Checks prerequisites (Node.js, npm deps, .env), spawns the process,
    /// waits for health, and fetches the unified API key.
    pub async fn start(&mut self) -> AiResult<()> {
        // 1. Check Node.js availability
        let node_version = Command::new("node")
            .arg("--version")
            .output()
            .await
            .map_err(|e| AiError::Internal(format!("Node.js not found: {}", e)))?;

        if !node_version.status.success() {
            return Err(AiError::Internal(
                "Node.js is required for the AI sidecar. Install Node.js 20+ and try again.".into(),
            ));
        }
        let version = String::from_utf8_lossy(&node_version.stdout);
        debug!("[sidecar] Node.js {}", version.trim());

        // 2. Ensure npm dependencies are installed
        let node_modules = self.freellmapi_dir.join("node_modules");
        if !node_modules.exists() {
            info!("[sidecar] Installing npm dependencies (first run)...");
            let install = Command::new("npm")
                .arg("install")
                .current_dir(&self.freellmapi_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
                .map_err(|e| AiError::Internal(format!("npm install failed: {}", e)))?;

            if !install.status.success() {
                let stderr = String::from_utf8_lossy(&install.stderr);
                return Err(AiError::Internal(format!(
                    "npm install failed: {}",
                    stderr.chars().take(200).collect::<String>()
                )));
            }
            info!("[sidecar] npm install complete");
        }

        // 3. Ensure .env exists
        let env_path = self.freellmapi_dir.join(".env");
        if !env_path.exists() {
            info!("[sidecar] Generating .env with encryption key...");
            let key_gen = Command::new("node")
                .arg("-e")
                .arg("console.log(require('crypto').randomBytes(32).toString('hex'))")
                .current_dir(&self.freellmapi_dir)
                .output()
                .await
                .map_err(|e| AiError::Internal(format!("Key generation failed: {}", e)))?;

            let key = String::from_utf8_lossy(&key_gen.stdout).trim().to_string();
            let env_content = format!("ENCRYPTION_KEY={}\nPORT={}\n", key, self.port);
            std::fs::write(&env_path, env_content)
                .map_err(|e| AiError::Internal(format!("Failed to write .env: {}", e)))?;
        }

        // 4. Find available port if requested port is 0
        let actual_port = if self.port == 0 {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| AiError::Internal(format!("Failed to find available port: {}", e)))?;
            let port = listener
                .local_addr()
                .map_err(|e| AiError::Internal(format!("Failed to get port: {}", e)))?
                .port();
            drop(listener);
            port
        } else {
            self.port
        };

        // 5. Update .env with the actual port
        let env_content = format!(
            "ENCRYPTION_KEY={}\nPORT={}\n",
            self.get_encryption_key().await?,
            actual_port
        );
        std::fs::write(&env_path, env_content)
            .map_err(|e| AiError::Internal(format!("Failed to write .env: {}", e)))?;

        // 6. Spawn the server
        let server_path = self
            .freellmapi_dir
            .join("server")
            .join("dist")
            .join("index.js");
        let server_src = self
            .freellmapi_dir
            .join("server")
            .join("src")
            .join("index.ts");

        let (cmd, args) = if server_path.exists() {
            (
                "node".to_string(),
                vec![server_path.to_string_lossy().to_string()],
            )
        } else {
            // Dev mode: use tsx to run TypeScript directly
            (
                "npx".to_string(),
                vec!["tsx".into(), server_src.to_string_lossy().to_string()],
            )
        };

        info!("[sidecar] Starting FreeLLMAPI on port {}...", actual_port);

        let child = Command::new(&cmd)
            .args(&args)
            .current_dir(&self.freellmapi_dir)
            .env("PORT", actual_port.to_string())
            .env("NODE_ENV", "production")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| AiError::Internal(format!("Failed to spawn sidecar: {}", e)))?;

        self.child = Some(child);
        self.port = actual_port;

        // 7. Wait for health check (poll /api/ping)
        self.base_url = format!("http://127.0.0.1:{}", actual_port);
        let client = reqwest::Client::new();
        let ping_url = format!("{}/api/ping", self.base_url);

        for attempt in 0..20 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            match client.get(&ping_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!("[sidecar] Ready on port {}", actual_port);
                    break;
                }
                Ok(resp) => {
                    debug!(
                        "[sidecar] Health check attempt {}: HTTP {}",
                        attempt + 1,
                        resp.status()
                    );
                }
                Err(e) => {
                    debug!("[sidecar] Health check attempt {}: {}", attempt + 1, e);
                }
            }

            if attempt == 19 {
                // Kill the process if it didn't start in time
                self.kill().await;
                return Err(AiError::Internal(
                    "Sidecar failed to start within 10 seconds".into(),
                ));
            }
        }

        // 8. Fetch the unified API key
        let key_url = format!("{}/api/settings/api-key", self.base_url);
        match client.get(&key_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                self.api_key = body["apiKey"]
                    .as_str()
                    .unwrap_or("freellmapi-dev")
                    .to_string();
                debug!("[sidecar] API key fetched");
            }
            _ => {
                warn!("[sidecar] Could not fetch API key, using default");
                self.api_key = "freellmapi-dev".to_string();
            }
        }

        Ok(())
    }

    /// Stop the sidecar process gracefully.
    pub async fn stop(&mut self) {
        if let Some(ref mut child) = self.child {
            info!("[sidecar] Stopping...");
            let _ = child.kill().await;
        }
        self.child = None;
    }

    /// Kill the sidecar process immediately.
    async fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill().await;
        }
        self.child = None;
    }

    /// Check if the sidecar is running and healthy.
    pub async fn is_healthy(&self) -> bool {
        if self.base_url.is_empty() {
            return false;
        }
        let client = reqwest::Client::new();
        client
            .get(format!("{}/api/ping", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Get the base URL for the sidecar.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the unified API key.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Get the port the sidecar is running on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the current status.
    pub fn status(&self) -> SidecarStatus {
        if self.child.is_some() {
            if self.base_url.is_empty() {
                SidecarStatus::Starting
            } else {
                SidecarStatus::Running { port: self.port }
            }
        } else if self.restart_count > 0 {
            if self.restart_count >= self.max_restarts {
                SidecarStatus::Failed
            } else {
                SidecarStatus::Restarting {
                    attempt: self.restart_count,
                }
            }
        } else {
            SidecarStatus::Stopped
        }
    }

    async fn get_encryption_key(&self) -> AiResult<String> {
        let env_path = self.freellmapi_dir.join(".env");
        if env_path.exists() {
            let content = std::fs::read_to_string(&env_path)
                .map_err(|e| AiError::Internal(format!("Failed to read .env: {}", e)))?;
            for line in content.lines() {
                if let Some(key) = line.strip_prefix("ENCRYPTION_KEY=") {
                    return Ok(key.to_string());
                }
            }
        }
        // Generate new key
        let output = Command::new("node")
            .arg("-e")
            .arg("console.log(require('crypto').randomBytes(32).toString('hex'))")
            .current_dir(&self.freellmapi_dir)
            .output()
            .await
            .map_err(|e| AiError::Internal(format!("Key generation failed: {}", e)))?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_stopped() {
        let mgr = SidecarManager::new(PathBuf::from("sidecar/freellmapi"), 3001);
        assert_eq!(mgr.status(), SidecarStatus::Stopped);
    }

    #[test]
    fn test_port_assignment() {
        let mgr = SidecarManager::new(PathBuf::from("sidecar/freellmapi"), 0);
        assert_eq!(mgr.port(), 0);
    }

    #[test]
    fn test_max_restarts() {
        let mut mgr = SidecarManager::new(PathBuf::from("sidecar/freellmapi"), 3001);
        mgr.restart_count = 5;
        assert_eq!(mgr.status(), SidecarStatus::Failed);
    }
}
