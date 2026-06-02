use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use tauri::{Emitter, State};

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
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitInfo {
    pub hash: String,
    pub message: String,
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBranchInfo {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStashEntry {
    pub index: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatchEvent {
    pub kind: String, // "created", "modified", "deleted"
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GutterDecoration {
    pub line_number: u32, // 1-based line number in current file
    pub kind: String,     // "modified", "added", "deleted"
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

/// Write content to a file. Returns true if the file was modified.
#[tauri::command]
fn write_file(path: String, content: String) -> Result<bool, String> {
    let old_content = std::fs::read_to_string(&path).ok();
    std::fs::write(&path, &content).map_err(|e| format!("Failed to write {}: {}", path, e))?;
    Ok(old_content.map(|c| c != content).unwrap_or(true))
}

/// Detect the language ID from a file path.
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

/// Get the current git branch name.
#[tauri::command]
fn get_git_branch(path: String) -> String {
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

/// Get the git repository root.
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

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get git status.
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
        let x = line.as_bytes()[0] as char;
        let y = line.as_bytes()[1] as char;
        let rest = &line[3..];

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
        if x != ' ' && x != '?' {
            status_str.push(x);
        }
        if y != ' ' {
            status_str.push(y);
        }
        if status_str.is_empty() {
            status_str = "??".to_string();
        }

        entries.push(GitFileStatus {
            path: file_path,
            status: status_str,
            staged: x != ' ',
            original_path,
        });
    }

    Ok(entries)
}

/// Stage a file.
#[tauri::command]
fn git_stage_file(path: String, file_path: String) -> Result<(), String> {
    run_git_command(path, &["add", "--", &file_path])
}

/// Stage all files.
#[tauri::command]
fn git_stage_all(path: String) -> Result<(), String> {
    run_git_command(path, &["add", "-A"])
}

/// Unstage a file.
#[tauri::command]
fn git_unstage_file(path: String, file_path: String) -> Result<(), String> {
    run_git_command(path, &["reset", "HEAD", "--", &file_path])
}

/// Discard changes in a file.
#[tauri::command]
fn git_discard_file(path: String, file_path: String) -> Result<(), String> {
    run_git_command(path, &["checkout", "--", &file_path])
}

/// Create a commit.
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

    let log_output = Command::new("git")
        .args(["-C", &path, "log", "-1", "--format=%H%n%s%n%an"])
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

/// Show diff for a file.
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

// ---------------------------------------------------------------------------
// Git Branch Commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn git_list_branches(path: String) -> Result<Vec<GitBranchInfo>, String> {
    let output = Command::new("git")
        .args(["-c", "color.ui=never", "-C", &path, "branch",
               "--format=%(refname:short)|%(upstream:short)"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git branch error: {}", stderr));
    }

    let current = get_git_branch(path.clone());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut branches = Vec::new();

    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, '|').collect();
        let name = parts[0].trim().to_string();
        let upstream = parts.get(1).filter(|s| !s.is_empty()).map(|s| s.to_string());
        branches.push(GitBranchInfo { name: name.clone(), current: name == current, upstream });
    }

    Ok(branches)
}

#[tauri::command]
fn git_switch_branch(path: String, branch_name: String, create_new: bool) -> Result<(), String> {
    let mut args = vec!["-C", &path, "switch"];
    if create_new {
        args.push("-c");
    }
    args.push(&branch_name);
    run_git_command_raw(&args)
}

#[tauri::command]
fn git_create_branch(path: String, branch_name: String, base_branch: Option<String>) -> Result<(), String> {
    let mut args = vec!["-C", &path, "switch", "-c", &branch_name];
    if let Some(base) = &base_branch {
        args.push(base);
    }
    run_git_command_raw(&args)
}

// ---------------------------------------------------------------------------
// Git Stash Commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn git_stash_push(path: String, message: Option<String>) -> Result<(), String> {
    let mut args = vec!["-C", &path, "stash", "push"];
    if let Some(msg) = &message {
        args.push("-m");
        args.push(msg);
    }
    run_git_command_raw(&args)
}

#[tauri::command]
fn git_stash_list(path: String) -> Result<Vec<GitStashEntry>, String> {
    let output = Command::new("git")
        .args(["-C", &path, "stash", "list", "--format=%gd|%s"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut stashes = Vec::new();

    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, '|').collect();
        let ref_str = parts[0].trim().to_string();
        let message = parts.get(1).unwrap_or(&"").trim().to_string();
        let index = ref_str
            .strip_prefix("stash@{")
            .and_then(|s| s.strip_suffix('}'))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        stashes.push(GitStashEntry { index, message });
    }

    Ok(stashes)
}

#[tauri::command]
fn git_stash_pop(path: String, index: Option<usize>) -> Result<(), String> {
    let mut args: Vec<String> = vec!["-C".into(), path.clone(), "stash".into(), "pop".into()];
    if let Some(i) = index {
        let stash_ref = format!("stash@{{{}}}", i);
        args.push(stash_ref);
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_git_command_raw(&arg_refs)
}

#[tauri::command]
fn git_stash_drop(path: String, index: usize) -> Result<(), String> {
    let args: Vec<String> = vec![
        "-C".into(), path.into(), "stash".into(), "drop".into(),
        format!("stash@{{{}}}", index),
    ];
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_git_command_raw(&arg_refs)
}

// ---------------------------------------------------------------------------
// Search Command
// ---------------------------------------------------------------------------

/// Search files in the workspace.
/// Tracks git grep first; falls back to grep -rI for untracked files.
#[tauri::command]
fn search_files(path: String, query: String, max_results: Option<usize>) -> Result<Vec<SearchResult>, String> {
    let max = max_results.unwrap_or(100);

    // 1. Try git grep (only searches tracked files, but fast and respects .gitignore)
    let mut results = search_with_git_grep(&path, &query, max)?;

    // 2. If git grep returned nothing, try grep -rI for untracked files
    if results.is_empty() && query.len() >= 2 {
        if let Ok(grep_results) = search_with_grep(&path, &query, max) {
            results = grep_results;
        }
    }

    Ok(results)
}

fn search_with_git_grep(path: &str, query: &str, max: usize) -> Result<Vec<SearchResult>, String> {
    let output = Command::new("git")
        .args(["-C", path, "grep", "-n", "-I", "--max-count", &max.to_string(), query])
        .output()
        .map_err(|e| format!("Failed to search: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for line in stdout.lines() {
        // Format: path:line:content (without --column flag for simplicity)
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        if parts.len() >= 3 {
            if let Ok(line_num) = parts[1].parse::<usize>() {
                let file_path = if parts[0].starts_with('/') || parts[0].contains('/') {
                    parts[0].to_string()
                } else {
                    parts[0].to_string()
                };
                results.push(SearchResult {
                    path: file_path,
                    line: line_num,
                    text: parts[2].to_string(),
                });
            }
        }
    }

    Ok(results)
}

fn search_with_grep(path: &str, query: &str, max: usize) -> Result<Vec<SearchResult>, String> {
    let output = Command::new("grep")
        .args([
            "-rnI",
            "--exclude-dir=.git",
            "--exclude-dir=node_modules",
            "--exclude-dir=target",
            "--max-count",
            &max.to_string(),
            query,
            path,
        ])
        .output()
        .map_err(|e| format!("Failed to run grep: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for line in stdout.lines() {
        // Format: path:line:content
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        if parts.len() >= 3 {
            if let Ok(line_num) = parts[1].parse::<usize>() {
                let file_path = parts[0].to_string();
                // Strip the absolute search path prefix for relative paths
                let rel_path = if file_path.starts_with(path) {
                    file_path[path.len()..].trim_start_matches('/').to_string()
                } else {
                    file_path
                };
                results.push(SearchResult {
                    path: rel_path,
                    line: line_num,
                    text: parts[2].to_string(),
                });
            }
        }
    }

    Ok(results)
}

// ---------------------------------------------------------------------------
// File Watcher
// ---------------------------------------------------------------------------

/// Start watching a directory for file changes. Emits "file-changed" events.
/// Uses raw notify crate; debouncing is done on the frontend (500ms).
#[tauri::command]
fn start_file_watcher(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use notify::{EventKind, RecursiveMode, Watcher};

    let app_handle = app.clone();
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let _ = tx.send(res);
    })
    .map_err(|e| format!("Failed to create watcher: {}", e))?;

    watcher
        .watch(Path::new(&path), RecursiveMode::Recursive)
        .map_err(|e| format!("Failed to watch path: {}", e))?;

    // Move watcher + rx into a thread to keep them alive for app lifetime
    std::thread::spawn(move || {
        let _keep_alive = watcher;
        while let Ok(Ok(event)) = rx.recv() {
            let kind = match event.kind {
                EventKind::Create(_) => "created",
                EventKind::Modify(_) => "modified",
                EventKind::Remove(_) => "deleted",
                _ => continue,
            };
            for event_path in &event.paths {
                // Skip .git directory changes to avoid noise
                let path_str = event_path.to_string_lossy();
                if path_str.contains("/.git/") || path_str.ends_with("/.git") {
                    continue;
                }
                let _ = app_handle.emit("file-changed", FileWatchEvent {
                    kind: kind.to_string(),
                    path: path_str.to_string(),
                });
            }
        }
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// AI & Utility Commands
// ---------------------------------------------------------------------------

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

#[tauri::command]
fn get_current_dir() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string())
}

#[tauri::command]
async fn check_ai_health() -> bool {
    let client = ai::FreeLlmClient::localhost();
    client.health_check().await
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn run_git_command(path: String, args: &[&str]) -> Result<(), String> {
    let mut cmd_args = vec!["-C", &path];
    cmd_args.extend_from_slice(args);

    let output = Command::new("git")
        .args(&cmd_args)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git error: {}", stderr));
    }
    Ok(())
}

fn run_git_command_raw(args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git error: {}", stderr));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Git Gutter Decorations
// ---------------------------------------------------------------------------

/// Get per-line gutter decoration info for a specific file.
/// Parses `git diff` to find modified/added/deleted lines.
#[tauri::command]
fn git_file_gutter(path: String, file_path: String) -> Result<Vec<GutterDecoration>, String> {
    let mut decorations = Vec::new();

    // Parse unstaged diff
    let output = Command::new("git")
        .args(["-C", &path, "diff", "--unified=0", "--", &file_path])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if output.status.success() {
        let diff = String::from_utf8_lossy(&output.stdout);
        decorations.extend(parse_diff_gutter(&diff));
    }

    // Parse staged diff too
    let staged_output = Command::new("git")
        .args(["-C", &path, "diff", "--cached", "--unified=0", "--", &file_path])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if staged_output.status.success() {
        let diff = String::from_utf8_lossy(&staged_output.stdout);
        decorations.extend(parse_diff_gutter(&diff));
    }

    // Deduplicate: if a line appears in both, the last entry wins (staged > unstaged)
    decorations.sort_by_key(|d| d.line_number);
    decorations.dedup_by_key(|d| d.line_number);

    Ok(decorations)
}

fn parse_diff_gutter(diff: &str) -> Vec<GutterDecoration> {
    let mut decorations = Vec::new();
    let mut new_line: u32 = 0;
    let mut has_removed_before_add = false;

    for line in diff.lines() {
        if let Some(hunk) = line.strip_prefix("@@ ") {
            // Parse @@ -old_start,old_count +new_start,new_count @@
            let parts: Vec<&str> = hunk.split_whitespace().collect();
            if let Some(new_part) = parts.get(1) {
                if let Some(start) = new_part.strip_prefix('+') {
                    new_line = start.split(',').next()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    has_removed_before_add = false;
                }
            }
        } else if line.starts_with('+') && !line.starts_with("+++") && new_line > 0 {
            // Decide if this is "modified" (had preceding - lines) or "added"
            let kind = if has_removed_before_add { "modified" } else { "added" };
            decorations.push(GutterDecoration { line_number: new_line, kind: kind.to_string() });
            new_line += 1;
            has_removed_before_add = false;
        } else if line.starts_with('-') && !line.starts_with("---") && new_line > 0 {
            decorations.push(GutterDecoration { line_number: new_line, kind: "deleted".to_string() });
            has_removed_before_add = true;
        } else if !line.starts_with("---") && !line.starts_with("+++") && !line.starts_with("@@") && new_line > 0 {
            new_line += 1;
            has_removed_before_add = false;
        }
    }

    decorations
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
            git_file_gutter,
            git_list_branches,
            git_switch_branch,
            git_create_branch,
            git_stash_push,
            git_stash_list,
            git_stash_pop,
            git_stash_drop,
            search_files,
            start_file_watcher,
            get_current_dir,
            chat_completion,
            check_ai_health,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Aurora");
}
