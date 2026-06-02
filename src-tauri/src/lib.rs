use serde::{Deserialize, Serialize};
use std::path::Path;
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
            get_current_dir,
            chat_completion,
            check_ai_health,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Aurora");
}
