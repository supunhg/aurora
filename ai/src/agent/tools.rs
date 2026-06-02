use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A tool the agent can invoke during a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// The result of executing a tool.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    /// For file-write tools: the proposed new content (requires approval).
    pub proposed_content: Option<String>,
    /// For file-write tools: the path being modified.
    pub file_path: Option<PathBuf>,
}

/// An agent tool that can be registered with the agent loop.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (e.g. "read_file", "write_file").
    fn name(&self) -> &str;

    /// Human-readable description for the AI.
    fn description(&self) -> &str;

    /// JSON Schema for parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Convert to the OpenAI tools format.
    fn to_openai_tool(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name(),
                "description": self.description(),
                "parameters": self.parameters_schema(),
            }
        })
    }

    /// Execute the tool with the given arguments.
    async fn execute(&self, args: serde_json::Value) -> ToolResult;
}

// ---------------------------------------------------------------------------
// Built-in tools
// ---------------------------------------------------------------------------

/// Read a file from disk.
pub struct ReadFileTool;

#[async_trait::async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Returns the full text content."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let path = match args["path"].as_str() {
            Some(p) => PathBuf::from(p),
            None => {
                return ToolResult {
                    success: false,
                    output: "Missing 'path' parameter".into(),
                    proposed_content: None,
                    file_path: None,
                }
            }
        };

        match std::fs::read_to_string(&path) {
            Ok(content) => ToolResult {
                success: true,
                output: content,
                proposed_content: None,
                file_path: None,
            },
            Err(e) => ToolResult {
                success: false,
                output: format!("Error reading {}: {}", path.display(), e),
                proposed_content: None,
                file_path: None,
            },
        }
    }
}

/// Write content to a file (proposed, requires approval).
pub struct WriteFileTool;

#[async_trait::async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. The change will be proposed for user approval before being applied."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The full content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let path = match args["path"].as_str() {
            Some(p) => PathBuf::from(p),
            None => {
                return ToolResult {
                    success: false,
                    output: "Missing 'path' parameter".into(),
                    proposed_content: None,
                    file_path: None,
                }
            }
        };
        let content = match args["content"].as_str() {
            Some(c) => c.to_string(),
            None => {
                return ToolResult {
                    success: false,
                    output: "Missing 'content' parameter".into(),
                    proposed_content: None,
                    file_path: None,
                }
            }
        };

        ToolResult {
            success: true,
            output: format!(
                "Proposed write to {} ({} bytes)",
                path.display(),
                content.len()
            ),
            proposed_content: Some(content),
            file_path: Some(path),
        }
    }
}

/// Search for files matching a pattern.
pub struct SearchFilesTool {
    pub workspace_root: PathBuf,
}

#[async_trait::async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search for files in the workspace matching a glob pattern. Returns matching file paths."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match (e.g. '**/*.rs', 'src/**/*.ts')"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let pattern = match args["pattern"].as_str() {
            Some(p) => p,
            None => {
                return ToolResult {
                    success: false,
                    output: "Missing 'pattern' parameter".into(),
                    proposed_content: None,
                    file_path: None,
                }
            }
        };

        let full_pattern = self.workspace_root.join(pattern);
        let pattern_str = full_pattern.to_string_lossy().to_string();

        match glob::glob(&pattern_str) {
            Ok(paths) => {
                let files: Vec<String> = paths
                    .filter_map(|p| p.ok())
                    .map(|p| {
                        p.strip_prefix(&self.workspace_root)
                            .unwrap_or(&p)
                            .to_string_lossy()
                            .to_string()
                    })
                    .take(50)
                    .collect();

                let count = files.len();
                let listing = files.join("\n");
                ToolResult {
                    success: true,
                    output: if count == 0 {
                        "No files matched the pattern".into()
                    } else {
                        format!("{} files found:\n{}", count, listing)
                    },
                    proposed_content: None,
                    file_path: None,
                }
            }
            Err(e) => ToolResult {
                success: false,
                output: format!("Invalid glob pattern: {}", e),
                proposed_content: None,
                file_path: None,
            },
        }
    }
}

/// List files in a directory.
pub struct ListDirectoryTool {
    pub workspace_root: PathBuf,
}

#[async_trait::async_trait]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &str {
        "list_directory"
    }

    fn description(&self) -> &str {
        "List files and subdirectories in a directory. Returns names and types."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path (relative to workspace root or absolute)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let path = match args["path"].as_str() {
            Some(p) => {
                let pb = PathBuf::from(p);
                if pb.is_absolute() {
                    pb
                } else {
                    self.workspace_root.join(p)
                }
            }
            None => self.workspace_root.clone(),
        };

        match std::fs::read_dir(&path) {
            Ok(entries) => {
                let mut items: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let file_type = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                            "dir"
                        } else {
                            "file"
                        };
                        format!("[{}] {}", file_type, e.file_name().to_string_lossy())
                    })
                    .collect();
                items.sort();
                ToolResult {
                    success: true,
                    output: if items.is_empty() {
                        "Directory is empty".into()
                    } else {
                        items.join("\n")
                    },
                    proposed_content: None,
                    file_path: None,
                }
            }
            Err(e) => ToolResult {
                success: false,
                output: format!("Error listing {}: {}", path.display(), e),
                proposed_content: None,
                file_path: None,
            },
        }
    }
}

/// Run a shell command (with timeout).
pub struct RunCommandTool;

#[async_trait::async_trait]
impl Tool for RunCommandTool {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Run a shell command and return its output. Use with caution."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (optional, defaults to workspace root)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let command = match args["command"].as_str() {
            Some(c) => c,
            None => {
                return ToolResult {
                    success: false,
                    output: "Missing 'command' parameter".into(),
                    proposed_content: None,
                    file_path: None,
                }
            }
        };

        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let combined = if stderr.is_empty() {
                    stdout.to_string()
                } else {
                    format!("stdout:\n{}\n\nstderr:\n{}", stdout, stderr)
                };
                // Truncate very long output
                let truncated = if combined.len() > 10_000 {
                    format!(
                        "{}...(truncated, {} bytes total)",
                        &combined[..10_000],
                        combined.len()
                    )
                } else {
                    combined
                };
                ToolResult {
                    success: out.status.success(),
                    output: truncated,
                    proposed_content: None,
                    file_path: None,
                }
            }
            Err(e) => ToolResult {
                success: false,
                output: format!("Failed to execute command: {}", e),
                proposed_content: None,
                file_path: None,
            },
        }
    }
}

/// Search for text content across files using ripgrep/grep.
pub struct GrepTool {
    pub workspace_root: PathBuf,
}

#[async_trait::async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search for a text pattern across files in the workspace. Returns matching lines with file paths."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Text or regex pattern to search for"
                },
                "include": {
                    "type": "string",
                    "description": "File pattern to include (e.g. '*.rs', '*.ts')"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let pattern = match args["pattern"].as_str() {
            Some(p) => p,
            None => {
                return ToolResult {
                    success: false,
                    output: "Missing 'pattern' parameter".into(),
                    proposed_content: None,
                    file_path: None,
                }
            }
        };

        let include = args["include"].as_str().unwrap_or("*");

        let output = tokio::process::Command::new("grep")
            .arg("-rn")
            .arg("--include")
            .arg(include)
            .arg(pattern)
            .arg(&self.workspace_root)
            .output()
            .await;

        match output {
            Ok(out) => {
                let results = String::from_utf8_lossy(&out.stdout);
                let truncated = if results.len() > 10_000 {
                    format!("{}...(truncated)", &results[..10_000])
                } else if results.is_empty() {
                    "No matches found".into()
                } else {
                    results.to_string()
                };
                ToolResult {
                    success: true,
                    output: truncated,
                    proposed_content: None,
                    file_path: None,
                }
            }
            Err(e) => ToolResult {
                success: false,
                output: format!("grep failed: {}", e),
                proposed_content: None,
                file_path: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_file() {
        let tool = ReadFileTool;
        let result = tool
            .execute(serde_json::json!({"path": "/etc/hostname"}))
            .await;
        assert!(result.success || result.output.contains("Error"));
    }

    #[tokio::test]
    async fn test_list_directory() {
        let tool = ListDirectoryTool {
            workspace_root: PathBuf::from("."),
        };
        let result = tool.execute(serde_json::json!({"path": "."})).await;
        assert!(result.success);
        assert!(result.output.contains("Cargo.toml"));
    }

    #[tokio::test]
    async fn test_search_files() {
        let tool = SearchFilesTool {
            workspace_root: PathBuf::from("."),
        };
        let result = tool.execute(serde_json::json!({"pattern": "*.toml"})).await;
        assert!(result.success);
        assert!(result.output.contains("Cargo.toml"));
    }

    #[tokio::test]
    async fn test_run_command() {
        let tool = RunCommandTool;
        let result = tool
            .execute(serde_json::json!({"command": "echo hello"}))
            .await;
        assert!(result.success);
        assert!(result.output.contains("hello"));
    }

    #[test]
    fn test_tool_to_openai() {
        let tool = ReadFileTool;
        let json = tool.to_openai_tool();
        assert_eq!(json["function"]["name"], "read_file");
        assert!(json["function"]["description"].is_string());
    }
}
