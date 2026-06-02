//! Aurora Editor — core text editing subsystem.
//!
//! This crate provides the foundational data structures for the editor:
//!
//! - **`buffer`** — The text buffer backed by `ropey::Rope` with undo/redo support,
//!   dirty tracking, and line offset caching.
//! - **`cursor`** — Multi-cursor management with selection support (anchor/head model).
//! - **`viewport`** — Virtual scrolling, scroll position, and visible line tracking.
//! - **`syntax`** — Syntax highlighting pipeline (rule-based fallback + tree-sitter optional).
//! - **`error`** — Typed error definitions for all editor operations.

pub mod buffer;
pub mod cursor;
pub mod error;
pub mod events;
pub mod syntax;
pub mod viewport;

pub use buffer::{Buffer, Delta};
pub use cursor::{Cursor, CursorSet};
pub use error::{EditorError, EditorResult};
pub use events::{EditorEvent, EventCollector};
pub use syntax::{
    default_dark_theme, default_light_theme, HighlightRange, HighlightSnapshot, ScopeClassifier,
    ScopeTheme, ThemeColor, PYTHON_KEYWORDS, RUST_KEYWORDS, TYPESCRIPT_KEYWORDS,
};
pub use viewport::{Viewport, ViewportConfig};

/// A combined editor state bundling the buffer, cursors, viewport, and
/// highlight snapshot together.
///
/// This is the primary high-level API for the editor. All editing operations
/// go through this struct, which coordinates cursor movements with buffer
/// modifications and viewport updates.
#[derive(Debug, Clone)]
pub struct Editor {
    /// The text buffer
    pub buffer: Buffer,
    /// The set of cursors (multi-cursor support)
    pub cursors: CursorSet,
    /// The viewport (virtual scrolling state)
    pub viewport: Viewport,
    /// The current syntax highlight snapshot
    pub highlights: HighlightSnapshot,
    /// The current theme (scope → color mappings)
    pub theme: ScopeTheme,
    /// Path to the currently open file (if any)
    pub file_path: Option<std::path::PathBuf>,
    /// Language identifier for LSP (e.g. "rust", "typescript")
    pub language_id: Option<String>,
    /// Document version counter (incremented on each edit)
    pub version: i32,
    /// Event collector for LSP/AI bridges to drain
    pub events: EventCollector,
}

impl Editor {
    /// Create a new editor with an empty buffer.
    pub fn new() -> Self {
        let buffer = Buffer::new();
        let line_count = buffer.len_lines();
        Editor {
            buffer,
            cursors: CursorSet::new(),
            viewport: Viewport::new(40, line_count),
            highlights: HighlightSnapshot::default(),
            theme: default_dark_theme(),
            file_path: None,
            language_id: None,
            version: 0,
            events: EventCollector::new(),
        }
    }

    /// Create a new editor with pre-populated text.
    pub fn from_text(text: &str) -> Self {
        let buffer = Buffer::from_text(text);
        let line_count = buffer.len_lines();
        Editor {
            buffer,
            cursors: CursorSet::new(),
            viewport: Viewport::new(40, line_count),
            highlights: HighlightSnapshot::default(),
            theme: default_dark_theme(),
            file_path: None,
            language_id: None,
            version: 0,
            events: EventCollector::new(),
        }
    }

    /// Create a new editor with a custom viewport height.
    pub fn with_viewport_height(text: &str, visible_lines: usize) -> Self {
        let buffer = Buffer::from_text(text);
        let line_count = buffer.len_lines();
        Editor {
            buffer,
            cursors: CursorSet::new(),
            viewport: Viewport::new(visible_lines, line_count),
            highlights: HighlightSnapshot::default(),
            theme: default_dark_theme(),
            file_path: None,
            language_id: None,
            version: 0,
            events: EventCollector::new(),
        }
    }

    /// Load text into the editor, replacing the current content.
    pub fn load_text(&mut self, text: &str) {
        // We need to rebuild the buffer from scratch since Buffer doesn't
        // have a clear-and-set operation. For now, we create a new buffer.
        let new_buffer = Buffer::from_text(text);
        let line_count = new_buffer.len_lines();
        self.buffer = new_buffer;
        self.viewport.set_total_lines(line_count);
        self.cursors = CursorSet::new();
        self.highlights = HighlightSnapshot::default();
    }

    /// Insert text at the primary cursor position.
    /// Returns the delta describing the edit.
    pub fn insert_at_cursor(&mut self, text: &str) -> EditorResult<Delta> {
        let pos = self.cursors.primary().position;
        let delta = self.buffer.insert(pos, text)?;
        self.cursors
            .apply_delta_offset(pos, delta.inserted.len() as isize);
        self.viewport.set_total_lines(self.buffer.len_lines());

        self.version += 1;
        if let Some(ref path) = &self.file_path {
            self.events.push(EditorEvent::BufferChanged {
                uri: path_to_uri(path),
                version: self.version,
                text: self.buffer.text(),
            });
            let (line, col) = self
                .buffer
                .byte_to_line_col(self.cursors.primary().position)
                .unwrap_or((0, 0));
            self.events.push(EditorEvent::CursorMoved {
                uri: path_to_uri(path),
                line: line as u32,
                column: col as u32,
            });
        }

        Ok(delta)
    }

    /// Delete the selection at the primary cursor, or the character before
    /// the cursor if there's no selection (backspace behavior).
    pub fn backspace(&mut self) -> EditorResult<Delta> {
        let cursor = *self.cursors.primary();
        let result = if cursor.is_collapsed() && cursor.position > 0 {
            let pos = cursor.position;
            let d = self.buffer.delete(pos - 1, pos)?;
            self.cursors.apply_delta_offset(pos - 1, -1);
            self.cursors.primary_mut().set_position(pos - 1);
            self.viewport.set_total_lines(self.buffer.len_lines());
            Ok(d)
        } else if !cursor.is_collapsed() {
            let (start, end) = cursor.selection_range();
            let d = self.buffer.delete(start, end)?;
            self.cursors
                .apply_delta_offset(start, -(d.deleted.len() as isize));
            self.cursors.primary_mut().set_position(start);
            self.viewport.set_total_lines(self.buffer.len_lines());
            Ok(d)
        } else {
            Ok(Delta::new(0, String::new(), String::new()))
        };

        if result.is_ok() && !result.as_ref().unwrap().is_noop() {
            self.version += 1;
            self.emit_buffer_changed();
        }
        result
    }

    /// Delete the selection at the primary cursor, or the character after
    /// the cursor if there's no selection (delete key behavior).
    pub fn delete_forward(&mut self) -> EditorResult<Delta> {
        let cursor = *self.cursors.primary();
        let result = if cursor.is_collapsed() && cursor.position < self.buffer.len_bytes() {
            let pos = cursor.position;
            let d = self.buffer.delete(pos, pos + 1)?;
            self.cursors.apply_delta_offset(pos, -1);
            self.viewport.set_total_lines(self.buffer.len_lines());
            Ok(d)
        } else if !cursor.is_collapsed() {
            let (start, end) = cursor.selection_range();
            let d = self.buffer.delete(start, end)?;
            self.cursors
                .apply_delta_offset(start, -(d.deleted.len() as isize));
            self.cursors.primary_mut().set_position(start);
            self.viewport.set_total_lines(self.buffer.len_lines());
            Ok(d)
        } else {
            Ok(Delta::new(0, String::new(), String::new()))
        };

        if result.is_ok() && !result.as_ref().unwrap().is_noop() {
            self.version += 1;
            self.emit_buffer_changed();
        }
        result
    }

    /// Move the primary cursor left by one character.
    pub fn cursor_left(&mut self) -> EditorResult<()> {
        let pos = self.cursors.primary().position;
        if pos > 0 {
            self.cursors.primary_mut().set_position(pos - 1);
        }
        Ok(())
    }

    /// Move the primary cursor right by one character.
    pub fn cursor_right(&mut self) -> EditorResult<()> {
        let pos = self.cursors.primary().position;
        if pos < self.buffer.len_bytes() {
            self.cursors.primary_mut().set_position(pos + 1);
        }
        Ok(())
    }

    /// Move the primary cursor up by one line.
    pub fn cursor_up(&mut self) -> EditorResult<()> {
        let cursor = self.cursors.primary();
        let (line, mut col) = self.buffer.byte_to_line_col(cursor.position)?;
        if let Some(pref) = cursor.preferred_col {
            col = pref;
        }
        if line > 0 {
            let target_line = line - 1;
            let target_line_len = self.buffer.line_len_chars(target_line)?;
            // If the target line is shorter, clamp to end
            if col >= target_line_len {
                col = target_line_len.saturating_sub(1);
            }
            let new_pos = self.buffer.line_col_to_byte(target_line, col)?;
            self.cursors.primary_mut().preferred_col = Some(col);
            self.cursors.primary_mut().set_position(new_pos);
            self.viewport.ensure_visible(target_line);
        }
        Ok(())
    }

    /// Move the primary cursor down by one line.
    pub fn cursor_down(&mut self) -> EditorResult<()> {
        let cursor = self.cursors.primary();
        let (line, mut col) = self.buffer.byte_to_line_col(cursor.position)?;
        if let Some(pref) = cursor.preferred_col {
            col = pref;
        }
        let total_lines = self.buffer.len_lines();
        if line + 1 < total_lines {
            let target_line = line + 1;
            let target_line_len = self.buffer.line_len_chars(target_line)?;
            if col >= target_line_len {
                col = target_line_len.saturating_sub(1);
            }
            let new_pos = self.buffer.line_col_to_byte(target_line, col)?;
            self.cursors.primary_mut().preferred_col = Some(col);
            self.cursors.primary_mut().set_position(new_pos);
            self.viewport.ensure_visible(target_line);
        }
        Ok(())
    }

    /// Move cursor to the beginning of the current line.
    pub fn cursor_home(&mut self) -> EditorResult<()> {
        let cursor = self.cursors.primary();
        let (line, _) = self.buffer.byte_to_line_col(cursor.position)?;
        let new_pos = self.buffer.line_col_to_byte(line, 0)?;
        self.cursors.primary_mut().set_position(new_pos);
        Ok(())
    }

    /// Move cursor to the end of the current line (before the newline, if any).
    pub fn cursor_end(&mut self) -> EditorResult<()> {
        let cursor = self.cursors.primary();
        let (line, _) = self.buffer.byte_to_line_col(cursor.position)?;
        let total_lines = self.buffer.len_lines();
        let line_len = self.buffer.line_len_chars(line)?;
        // For all lines except the last, the line length includes the trailing newline.
        // Cursor should stop *before* the newline.
        let end_col = if line + 1 < total_lines && line_len > 0 {
            line_len - 1
        } else {
            line_len
        };
        let new_pos = self.buffer.line_col_to_byte(line, end_col)?;
        self.cursors.primary_mut().set_position(new_pos);
        Ok(())
    }

    /// Move cursor to the beginning of the buffer.
    pub fn cursor_top(&mut self) -> EditorResult<()> {
        self.cursors.primary_mut().set_position(0);
        self.viewport.scroll_to_top();
        Ok(())
    }

    /// Move cursor to the end of the buffer.
    pub fn cursor_bottom(&mut self) -> EditorResult<()> {
        let pos = self.buffer.len_bytes();
        self.cursors.primary_mut().set_position(pos);
        self.viewport.scroll_to_bottom();
        Ok(())
    }

    /// Undo the last edit.
    pub fn undo(&mut self) -> EditorResult<Delta> {
        let delta = self.buffer.undo()?;
        // Adjust cursor position if the undo affects the cursor location
        self.cursors
            .apply_delta_offset(delta.position, -(delta.net_change()));
        self.viewport.set_total_lines(self.buffer.len_lines());
        Ok(delta)
    }

    /// Redo the last undone edit.
    pub fn redo(&mut self) -> EditorResult<Delta> {
        let delta = self.buffer.redo()?;
        self.cursors
            .apply_delta_offset(delta.position, delta.net_change());
        self.viewport.set_total_lines(self.buffer.len_lines());
        Ok(delta)
    }

    /// Switch to a dark theme.
    pub fn use_dark_theme(&mut self) {
        self.theme = default_dark_theme();
    }

    /// Switch to a light theme.
    pub fn use_light_theme(&mut self) {
        self.theme = default_light_theme();
    }

    /// Perform syntax highlighting on the visible range using the rule-based
    /// classifier. For a real editor, this would call tree-sitter on a
    /// background thread.
    pub fn highlight_visible_range(&mut self, language_keywords: &[&str]) {
        let (start_line, end_line) = self.viewport.render_range();
        let mut ranges = Vec::new();

        for line_idx in start_line..end_line {
            if let Ok(line) = self.buffer.get_line(line_idx) {
                let line_start_byte = match self.buffer.line_col_to_byte(line_idx, 0) {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                let line_ranges =
                    ScopeClassifier::classify_line(&line, line_start_byte, language_keywords);
                ranges.extend(line_ranges);
            }
        }

        self.highlights = HighlightSnapshot {
            ranges,
            buffer_version: 0, // placeholder
        };
    }

    // ------------------------------------------------------------------
    // File I/O
    // ------------------------------------------------------------------

    /// Open a file from disk into the editor buffer.
    pub fn open_file(&mut self, path: &std::path::Path) -> EditorResult<()> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            EditorError::Internal(format!("Failed to read {}: {}", path.display(), e))
        })?;
        let language_id = detect_language(path);
        let uri = path_to_uri(path);

        self.load_text(&text);
        self.file_path = Some(path.to_path_buf());
        self.language_id = Some(language_id.clone());
        self.version = 1;

        self.events.push(EditorEvent::FileOpened {
            uri,
            language_id,
            text,
        });
        Ok(())
    }

    /// Save the editor buffer to a file.
    pub fn save_file(&self, path: &std::path::Path) -> EditorResult<()> {
        std::fs::write(path, self.buffer.text()).map_err(|e| {
            EditorError::Internal(format!("Failed to write {}: {}", path.display(), e))
        })
    }

    // ------------------------------------------------------------------
    // Selection & editing operations
    // ------------------------------------------------------------------

    /// Select all text in the buffer.
    pub fn select_all(&mut self) {
        let end = self.buffer.len_bytes();
        self.cursors = CursorSet::at_position(0);
        self.cursors.primary_mut().start_selection(0);
        self.cursors.primary_mut().extend_selection(end);
    }

    /// Delete the current selection (or do nothing if no selection).
    pub fn delete_selection(&mut self) -> EditorResult<Delta> {
        let cursor = *self.cursors.primary();
        if !cursor.is_collapsed() {
            let (start, end) = cursor.selection_range();
            let d = self.buffer.delete(start, end)?;
            self.cursors
                .apply_delta_offset(start, -(d.deleted.len() as isize));
            self.cursors.primary_mut().set_position(start);
            self.viewport.set_total_lines(self.buffer.len_lines());
            Ok(d)
        } else {
            Ok(Delta::new(0, String::new(), String::new()))
        }
    }

    /// Get the currently selected text, if any.
    pub fn selected_text(&self) -> Option<String> {
        let cursor = self.cursors.primary();
        if cursor.is_collapsed() {
            None
        } else {
            let (start, end) = cursor.selection_range();
            self.buffer.slice(start, end).ok()
        }
    }

    /// Delete the current line.
    pub fn delete_line(&mut self) -> EditorResult<Delta> {
        let cursor = *self.cursors.primary();
        let (line, _) = self.buffer.byte_to_line_col(cursor.position)?;
        let line_start = self.buffer.line_col_to_byte(line, 0)?;
        let total_lines = self.buffer.len_lines();
        let line_end = if line + 1 < total_lines {
            self.buffer.line_col_to_byte(line + 1, 0)?
        } else {
            self.buffer.len_bytes()
        };
        let d = self.buffer.delete(line_start, line_end)?;
        self.cursors.primary_mut().set_position(line_start);
        self.viewport.set_total_lines(self.buffer.len_lines());
        Ok(d)
    }

    /// Duplicate the current line below.
    pub fn duplicate_line(&mut self) -> EditorResult<Delta> {
        let cursor = *self.cursors.primary();
        let (line, _) = self.buffer.byte_to_line_col(cursor.position)?;
        let total_lines = self.buffer.len_lines();
        let line_start = self.buffer.line_col_to_byte(line, 0)?;
        let line_end = if line + 1 < total_lines {
            self.buffer.line_col_to_byte(line + 1, 0)?
        } else {
            self.buffer.len_bytes()
        };
        let line_text = self.buffer.slice(line_start, line_end)?;
        let d = self.buffer.insert(line_end, &line_text)?;
        self.cursors
            .apply_delta_offset(line_end, d.inserted.len() as isize);
        self.viewport.set_total_lines(self.buffer.len_lines());
        Ok(d)
    }

    /// Join the current line with the next line.
    pub fn join_lines(&mut self) -> EditorResult<Delta> {
        let cursor = *self.cursors.primary();
        let (line, _) = self.buffer.byte_to_line_col(cursor.position)?;
        let total_lines = self.buffer.len_lines();
        if line + 1 >= total_lines {
            return Ok(Delta::new(0, String::new(), String::new()));
        }
        let line_len = self.buffer.line_len_chars(line)?;
        // line_len includes the trailing newline for non-last lines,
        // so the newline is at column (line_len - 1).
        let newline_byte = self.buffer.line_col_to_byte(line, line_len - 1)?;
        // Delete the newline character
        let d = self.buffer.delete(newline_byte, newline_byte + 1)?;
        self.cursors.primary_mut().set_position(newline_byte);
        self.viewport.set_total_lines(self.buffer.len_lines());
        Ok(d)
    }

    /// Indent the current line (insert tab_width spaces at line start).
    pub fn indent_line(&mut self, tab_width: usize) -> EditorResult<Delta> {
        let cursor = *self.cursors.primary();
        let (line, _) = self.buffer.byte_to_line_col(cursor.position)?;
        let line_start = self.buffer.line_col_to_byte(line, 0)?;
        let indent = " ".repeat(tab_width);
        let d = self.buffer.insert(line_start, &indent)?;
        self.cursors
            .apply_delta_offset(line_start, d.inserted.len() as isize);
        Ok(d)
    }

    /// Outdent the current line (remove up to tab_width leading spaces).
    pub fn outdent_line(&mut self, tab_width: usize) -> EditorResult<Delta> {
        let cursor = *self.cursors.primary();
        let (line, _) = self.buffer.byte_to_line_col(cursor.position)?;
        let line_start = self.buffer.line_col_to_byte(line, 0)?;
        let line_text = self.buffer.get_line(line)?;
        let spaces = line_text.chars().take_while(|c| *c == ' ').count();
        let remove = spaces.min(tab_width);
        if remove == 0 {
            return Ok(Delta::new(0, String::new(), String::new()));
        }
        let d = self.buffer.delete(line_start, line_start + remove)?;
        self.cursors.primary_mut().set_position(line_start);
        Ok(d)
    }

    /// Toggle line comment for the current line.
    pub fn toggle_line_comment(&mut self, comment_prefix: &str) -> EditorResult<Delta> {
        let cursor = *self.cursors.primary();
        let (line, _) = self.buffer.byte_to_line_col(cursor.position)?;
        let line_start = self.buffer.line_col_to_byte(line, 0)?;
        let line_text = self.buffer.get_line(line)?;
        let trimmed = line_text.trim_start();
        if trimmed.starts_with(comment_prefix) {
            // Remove the comment prefix
            let prefix_len = line_text.len() - trimmed.len();
            let comment_end = line_start + prefix_len + comment_prefix.len();
            let d = self.buffer.delete(line_start, comment_end)?;
            self.cursors
                .apply_delta_offset(line_start, -(d.deleted.len() as isize));
            self.cursors.primary_mut().set_position(line_start);
            Ok(d)
        } else {
            // Add the comment prefix
            let d = self.buffer.insert(line_start, comment_prefix)?;
            self.cursors
                .apply_delta_offset(line_start, d.inserted.len() as isize);
            Ok(d)
        }
    }

    /// Move cursor to the start of the next word.
    pub fn cursor_word_right(&mut self) -> EditorResult<()> {
        let pos = self.cursors.primary().position;
        let text = self.buffer.text();
        let bytes = text.as_bytes();
        let len = bytes.len();
        if pos >= len {
            return Ok(());
        }
        let mut i = pos;
        // Skip non-word characters
        while i < len && !bytes[i].is_ascii_alphanumeric() && bytes[i] != b'_' {
            i += 1;
        }
        // Skip word characters
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        self.cursors.primary_mut().set_position(i);
        Ok(())
    }

    /// Move cursor to the start of the previous word.
    pub fn cursor_word_left(&mut self) -> EditorResult<()> {
        let pos = self.cursors.primary().position;
        if pos == 0 {
            return Ok(());
        }
        let text = self.buffer.text();
        let bytes = text.as_bytes();
        let mut i = pos;
        // Skip non-word characters backwards
        while i > 0 && !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_' {
            i -= 1;
        }
        // Skip word characters backwards
        while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
            i -= 1;
        }
        self.cursors.primary_mut().set_position(i);
        Ok(())
    }

    /// Delete the word before the cursor.
    pub fn delete_word_left(&mut self) -> EditorResult<Delta> {
        let pos = self.cursors.primary().position;
        if pos == 0 {
            return Ok(Delta::new(0, String::new(), String::new()));
        }
        let text = self.buffer.text();
        let bytes = text.as_bytes();
        let mut start = pos;
        // Skip non-word characters backwards
        while start > 0 && !bytes[start - 1].is_ascii_alphanumeric() && bytes[start - 1] != b'_' {
            start -= 1;
        }
        // Skip word characters backwards
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            start -= 1;
        }
        let d = self.buffer.delete(start, pos)?;
        self.cursors
            .apply_delta_offset(start, -(d.deleted.len() as isize));
        self.cursors.primary_mut().set_position(start);
        self.viewport.set_total_lines(self.buffer.len_lines());
        Ok(d)
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Emit a BufferChanged event if a file is open.
    fn emit_buffer_changed(&mut self) {
        if let (Some(ref path), Some(_)) = (&self.file_path, &self.language_id) {
            let (line, col) = self
                .buffer
                .byte_to_line_col(self.cursors.primary().position)
                .unwrap_or((0, 0));
            self.events.push(EditorEvent::BufferChanged {
                uri: path_to_uri(path),
                version: self.version,
                text: self.buffer.text(),
            });
            self.events.push(EditorEvent::CursorMoved {
                uri: path_to_uri(path),
                line: line as u32,
                column: col as u32,
            });
        }
    }

    /// Get the current file URI (if a file is open).
    pub fn file_uri(&self) -> Option<String> {
        self.file_path.as_ref().map(|p| path_to_uri(p))
    }
}

// ------------------------------------------------------------------
// Utility functions
// ------------------------------------------------------------------

/// Convert a file path to a file:// URI.
pub fn path_to_uri(path: &std::path::Path) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(target_os = "windows")]
    {
        format!("file:///{}", abs.to_string_lossy().replace('\\', "/"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("file://{}", abs.to_string_lossy())
    }
}

/// Detect language ID from file extension.
pub fn detect_language(path: &std::path::Path) -> String {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" => "c",
        "java" => "java",
        "rb" => "ruby",
        "php" => "php",
        "html" | "htm" => "html",
        "css" => "css",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" => "markdown",
        "sh" | "bash" => "shell",
        "sql" => "sql",
        "xml" => "xml",
        "zig" => "zig",
        "ex" | "exs" => "elixir",
        "hs" => "haskell",
        "lua" => "lua",
        "dart" => "dart",
        "swift" => "swift",
        "kt" => "kotlin",
        _ => "plaintext",
    }
    .to_string()
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_new() {
        let ed = Editor::new();
        assert!(ed.buffer.is_empty());
        assert_eq!(ed.cursors.len(), 1);
        assert_eq!(ed.cursors.primary().position, 0);
    }

    #[test]
    fn test_editor_insert() {
        let mut ed = Editor::new();
        ed.insert_at_cursor("hello").unwrap();
        assert_eq!(ed.buffer.text(), "hello");
        assert_eq!(ed.cursors.primary().position, 5);
    }

    #[test]
    fn test_editor_undo_redo() {
        let mut ed = Editor::new();
        ed.insert_at_cursor("hello").unwrap();
        ed.undo().unwrap();
        assert_eq!(ed.buffer.text(), "");
        ed.redo().unwrap();
        assert_eq!(ed.buffer.text(), "hello");
    }

    #[test]
    fn test_editor_backspace() {
        let mut ed = Editor::new();
        ed.insert_at_cursor("hello").unwrap();
        ed.backspace().unwrap();
        assert_eq!(ed.buffer.text(), "hell");
        assert_eq!(ed.cursors.primary().position, 4);
    }

    #[test]
    fn test_cursor_movement() {
        let mut ed = Editor::from_text("hello\nworld");
        ed.cursor_right().unwrap();
        assert_eq!(ed.cursors.primary().position, 1);
        ed.cursor_down().unwrap();
        let (line, col) = ed
            .buffer
            .byte_to_line_col(ed.cursors.primary().position)
            .unwrap();
        assert_eq!(line, 1);
        assert_eq!(col, 1);
    }

    #[test]
    fn test_cursor_home_end() {
        let mut ed = Editor::from_text("hello\nworld");
        ed.cursor_end().unwrap();
        // End of "hello" (before newline) is byte 5
        assert_eq!(ed.cursors.primary().position, 5);
        ed.cursor_home().unwrap();
        assert_eq!(ed.cursors.primary().position, 0);
    }

    #[test]
    fn test_viewport_updates() {
        let mut ed = Editor::from_text("line1\nline2\nline3\nline4\nline5\nline6");
        ed.cursor_bottom().unwrap();
        assert!(ed.viewport.is_at_bottom() || ed.viewport.first_line > 0);
    }

    #[test]
    fn test_highlight_visible() {
        let mut ed = Editor::from_text("fn main() {\n    let x = 42;\n}");
        ed.highlight_visible_range(RUST_KEYWORDS);
        assert!(!ed.highlights.ranges.is_empty());
    }

    #[test]
    fn test_load_text() {
        let mut ed = Editor::new();
        ed.load_text("new content");
        assert_eq!(ed.buffer.text(), "new content");
        assert_eq!(ed.cursors.primary().position, 0);
    }

    #[test]
    fn test_select_all() {
        let mut ed = Editor::from_text("hello world");
        ed.select_all();
        assert_eq!(ed.selected_text().as_deref(), Some("hello world"));
    }

    #[test]
    fn test_delete_line() {
        let mut ed = Editor::from_text("line1\nline2\nline3");
        ed.cursor_down().unwrap();
        ed.delete_line().unwrap();
        assert_eq!(ed.buffer.text(), "line1\nline3");
    }

    #[test]
    fn test_duplicate_line() {
        let mut ed = Editor::from_text("line1\nline2");
        ed.duplicate_line().unwrap();
        assert_eq!(ed.buffer.text(), "line1\nline1\nline2");
    }

    #[test]
    fn test_join_lines() {
        let mut ed = Editor::from_text("hello\nworld");
        ed.join_lines().unwrap();
        assert_eq!(ed.buffer.text(), "helloworld");
    }

    #[test]
    fn test_indent_outdent() {
        let mut ed = Editor::from_text("fn main()");
        ed.indent_line(4).unwrap();
        assert_eq!(ed.buffer.text(), "    fn main()");
        ed.outdent_line(4).unwrap();
        assert_eq!(ed.buffer.text(), "fn main()");
    }

    #[test]
    fn test_toggle_comment() {
        let mut ed = Editor::from_text("fn main()");
        ed.toggle_line_comment("// ").unwrap();
        assert_eq!(ed.buffer.text(), "// fn main()");
        ed.toggle_line_comment("// ").unwrap();
        assert_eq!(ed.buffer.text(), "fn main()");
    }

    #[test]
    fn test_word_navigation() {
        let mut ed = Editor::from_text("hello world foo");
        ed.cursor_word_right().unwrap();
        assert_eq!(ed.cursors.primary().position, 5);
        ed.cursor_word_right().unwrap();
        assert_eq!(ed.cursors.primary().position, 11);
        ed.cursor_word_left().unwrap();
        assert_eq!(ed.cursors.primary().position, 6);
    }

    #[test]
    fn test_delete_word_left() {
        let mut ed = Editor::from_text("hello world");
        ed.cursor_bottom().unwrap();
        ed.cursor_word_left().unwrap();
        // cursor should be at start of "world" (position 6)
        assert_eq!(ed.cursors.primary().position, 6);
        ed.delete_word_left().unwrap();
        assert_eq!(ed.buffer.text(), "world");
    }
}
