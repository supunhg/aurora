use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use tauri::State;

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

pub struct AppState {
    pub ai_client: Option<ai::FreeLlmClient>,
}

// ---------------------------------------------------------------------------
// Data types shared with frontend
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub depth: usize,
    pub expanded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileStatus {
    /// The file path relative to repo root
    pub path: String,
    /// Status code, e.g. "M", "A", "D", "R", "??", "MM", etc.
    pub status: String,
    /// Whether the change is staged
    pub staged: bool,
    /// For renames, the original path
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitInfo {
    pub hash: String,
    pub message: String,
    pub author: String,
}

// ---------------------------------------------------------------------------
// Tauri Commands
// ---------------------------------------------------------------------------

/// List a directory's contents, sorted with directories first.
#[tauri::command]
fn list_directory(path: String, depth: usize) -> Result<Vec<FileEntry>, String> {
    let dir = Path::new(&path);
    if !dir.is_dir() {
        return Err(format!("Not a directory: {}", path));
    }

    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(dir).map_err(|e| e.to_string())?;

    let mut sorted: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
    sorted.sort_by(|a, b| {
        let a_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        b_dir
            .cmp(&a_dir)
            .then_with(|| a.file_name().cmp(&b.file_name()))
    });

    for entry in sorted {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        entries.push(FileEntry {
            name,
            path: entry.path().to_string_lossy().to_string(),
            is_dir,
            depth,
            expanded: false,
        });
    }

    Ok(entries)
}

/// Read a file's contents as a string.
#[tauri::command]
fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path, e))
}

/// Write content to a file.
#[tauri::command]
fn write_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, &content).map_err(|e| format!("Failed to write {}: {}", path, e))
}

/// Detect the language ID from a file path (e.g., "rust", "typescript").
#[tauri::command]
fn detect_language(path: String) -> String {
    let p = Path::new(&path);
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" => "markdown",
        "html" => "html",
        "css" | "scss" | "sass" => "css",
        "c" | "h" => "c",
        "cpp" | "hpp" => "cpp",
        "java" => "java",
        "rb" => "ruby",
        "php" => "php",
        "sh" | "bash" => "shell",
        "sql" => "sql",
        "lua" => "lua",
        "dart" => "dart",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        _ => "plaintext",
    }
    .to_string()
}

/// Get the git branch for a given directory.
#[tauri::command]
fn get_git_branch(path: String) -> String {
    // Try git symbolic-ref first (handles detached HEAD, worktrees, etc.)
    let output = Command::new("git")
        .args(["-C", &path, "rev-parse", "--abbrev-ref", "HEAD"])
        .output();
    
    if let Ok(out) = output {
        if out.status.success() {
            let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if branch != "HEAD" && !branch.is_empty() {
                return branch;
            }
            return "HEAD (detached)".to_string();
        }
    }

    // Fallback: try to find .git/HEAD manually
    let head_path = Path::new(&path).join(".git").join("HEAD");
    if let Ok(content) = std::fs::read_to_string(head_path) {
        if let Some(ref_line) = content.lines().next() {
            if let Some(branch) = ref_line.strip_prefix("ref: refs/heads/") {
                return branch.to_string();
            }
        }
    }
    "main".to_string()
}

/// Get the git repository root directory.
#[tauri::command]
fn get_git_root(path: String) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &path, "rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Not a git repository: {}", stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string())
}

/// Get git status using `git status --porcelain`.
#[tauri::command]
fn git_status(path: String) -> Result<Vec<GitFileStatus>, String> {
    let output = Command::new("git")
        .args(["-C", &path, "status", "--porcelain", "-u"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git error: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();

    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }
        // First two chars are the status, e.g. "M " or " M" or "??" or "MM"
        let x = line.as_bytes()[0] as char; // index (staged) status
        let y = line.as_bytes()[1] as char; // worktree status
        let rest = &line[3..];

        // Handle renames: "R  oldname -> newname"
        let (file_path, original_path) = if x == 'R' || y == 'R' {
            if let Some(arrow_pos) = rest.find(" -> ") {
                let orig = rest[..arrow_pos].to_string();
                let new_path = rest[arrow_pos + 4..].to_string();
                (new_path, Some(orig))
            } else {
                (rest.to_string(), None)
            }
        } else {
            (rest.to_string(), None)
        };

        let mut status_str = String::new();
        // Build status string like git: "M", "A", "D", "??", "MM", etc.
        if x != ' ' && x != '?' {
            status_str.push(x);
        }
        if y != ' ' {
            status_str.push(y);
        }
        if status_str.is_empty() {
            // untracked
            status_str = "??".to_string();
        }

        // Staged if the index column has a change
        let staged = x != ' ';

        entries.push(GitFileStatus {
            path: file_path,
            status: status_str,
            staged,
            original_path,
        });
    }

    Ok(entries)
}

/// Stage a file (git add).
#[tauri::command]
fn git_stage_file(path: String, file_path: String) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", &path, "add", "--", &file_path])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git stage error: {}", stderr));
    }
    Ok(())
}

/// Stage all files.
#[tauri::command]
fn git_stage_all(path: String) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", &path, "add", "-A"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git stage all error: {}", stderr));
    }
    Ok(())
}

/// Unstage a file (git reset HEAD -- file).
#[tauri::command]
fn git_unstage_file(path: String, file_path: String) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", &path, "reset", "HEAD", "--", &file_path])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git unstage error: {}", stderr));
    }
    Ok(())
}

/// Discard changes in a file (git checkout -- file).
#[tauri::command]
fn git_discard_file(path: String, file_path: String) -> Result<(), String> {
    let output = Command::new("git")
        .args(["-C", &path, "checkout", "--", &file_path])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git discard error: {}", stderr));
    }
    Ok(())
}

/// Create a commit (git commit -m "message").
#[tauri::command]
fn git_commit(path: String, message: String) -> Result<GitCommitInfo, String> {
    let output = Command::new("git")
        .args(["-C", &path, "commit", "-m", &message])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git commit error: {}", stderr));
    }

    // Get the last commit hash
    let log_output = Command::new("git")
        .args([
            "-C",
            &path,
            "log",
            "-1",
            "--format=%H%n%s%n%an",
        ])
        .output()
        .map_err(|e| format!("Failed to get commit info: {}", e))?;

    let log_stdout = String::from_utf8_lossy(&log_output.stdout);
    let mut lines = log_stdout.lines();

    Ok(GitCommitInfo {
        hash: lines.next().unwrap_or("unknown").to_string(),
        message: lines.next().unwrap_or("").to_string(),
        author: lines.next().unwrap_or("unknown").to_string(),
    })
}

/// Get the diff for a specific file.
/// If staged is true, shows diff between HEAD and index (staged).
/// If staged is false, shows diff between index and worktree (unstaged).
#[tauri::command]
fn git_show_diff(path: String, file_path: String, staged: bool) -> Result<String, String> {
    let mut args = vec!["-C", &path, "diff"];
    if staged {
        args.push("--cached");
    }
    args.push("--");
    args.push(&file_path);

    let output = Command::new("git")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git diff error: {}", stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Send a chat message to the AI backend and get a response.
#[tauri::command]
async fn chat_completion(
    _state: State<'_, AppState>,
    model: String,
    messages: Vec<ChatMessage>,
) -> Result<String, String> {
    let client = ai::FreeLlmClient::localhost();

    let freellm_messages: Vec<ai::freellm::ChatMessage> = messages
        .into_iter()
        .map(|m| ai::freellm::ChatMessage {
            role: m.role,
            content: m.content,
        })
        .collect();

    let resp = client
        .chat_completion(&model, freellm_messages)
        .await
        .map_err(|e| format!("AI error: {}", e))?;

    Ok(resp
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_else(|| "No response".into()))
}

/// Get the current working directory.
#[tauri::command]
fn get_current_dir() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string())
}

/// Check if the AI sidecar is running.
#[tauri::command]
async fn check_ai_health() -> bool {
    let client = ai::FreeLlmClient::localhost();
    client.health_check().await
}

// ---------------------------------------------------------------------------
// Application entry
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState { ai_client: None })
        .invoke_handler(tauri::generate_handler![
            list_directory,
            read_file,
            write_file,
            detect_language,
            get_git_branch,
            get_git_root,
            git_status,
            git_stage_file,
            git_stage_all,
            git_unstage_file,
            git_discard_file,
            git_commit,
            git_show_diff,
            get_current_dir,
            chat_completion,
            check_ai_health,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Aurora");
}
