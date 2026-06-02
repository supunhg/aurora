//! The core text buffer backed by `ropey::Rope` with undo/redo support.
//!
//! ## Architecture
//!
//! - **Data Structure:** `ropey::Rope` for O(log n) insert/delete at any position
//! - **Undo Stack:** Stores `Delta` structs (compressed edit history)
//! - **Line Cache:** Cached line start offsets for O(1) line→char translation
//!
//! All operations that modify the buffer produce a `Delta` that can be
//! pushed onto the undo stack and reverted.

use crate::error::{EditorError, EditorResult};
use ropey::Rope;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Delta — a single reversible edit
// ---------------------------------------------------------------------------

/// A reversible edit operation.
///
/// Each edit stores the range of characters that were removed (if any)
/// and the text that was inserted in their place. Reverting the delta
/// swaps the two: removes the inserted text and re-inserts the removed range.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Delta {
    /// Byte offset in the buffer where the edit occurred
    pub position: usize,
    /// The text that was removed (empty for pure insertions)
    pub deleted: String,
    /// The text that was inserted (empty for pure deletions)
    pub inserted: String,
}

impl Delta {
    /// Create a new delta.
    pub fn new(position: usize, deleted: String, inserted: String) -> Self {
        Delta {
            position,
            deleted,
            inserted,
        }
    }

    /// Return the inverse delta (for undo).
    pub fn invert(&self) -> Self {
        Delta {
            position: self.position,
            deleted: self.inserted.clone(),
            inserted: self.deleted.clone(),
        }
    }

    /// Number of characters the buffer length changed by this delta.
    pub fn net_change(&self) -> isize {
        self.inserted.len() as isize - self.deleted.len() as isize
    }

    /// Whether this is a no-op (nothing inserted and nothing deleted).
    pub fn is_noop(&self) -> bool {
        self.deleted.is_empty() && self.inserted.is_empty()
    }
}

// ---------------------------------------------------------------------------
// CachedLineOffsets — fast line → char translation
// ---------------------------------------------------------------------------

/// Lazily-computed cache of line start byte offsets.
///
/// The cache is invalidated on any line count change and rebuilt on
/// the next access, keeping reads O(1) amortized.
#[derive(Debug, Clone)]
pub struct CachedLineOffsets {
    offsets: Vec<usize>,
    dirty: bool,
}

impl Default for CachedLineOffsets {
    fn default() -> Self {
        Self::new()
    }
}

impl CachedLineOffsets {
    pub fn new() -> Self {
        CachedLineOffsets {
            offsets: vec![0],
            dirty: false,
        }
    }

    /// Mark the cache as needing a rebuild (call after every edit).
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Rebuild offsets from the rope, if dirty.
    pub fn rebuild(&mut self, rope: &Rope) {
        if !self.dirty {
            return;
        }
        let len_lines = rope.len_lines();
        self.offsets.clear();
        self.offsets.reserve(len_lines);
        for i in 0..len_lines {
            self.offsets.push(rope.line_to_byte(i));
        }
        self.dirty = false;
    }

    /// Get the byte offset for line `idx` (0-based).
    /// Panics if the cache hasn't been rebuilt after the last invalidation.
    pub fn line_offset(&self, idx: usize) -> Option<usize> {
        self.offsets.get(idx).copied()
    }

    /// Total number of cached lines.
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Buffer — the main editable text model
// ---------------------------------------------------------------------------

/// The core text buffer, backed by `ropey::Rope` with undo/redo support.
///
/// ## Undo / Redo
///
/// Each `Delta` is pushed onto the undo stack. Undo pops the stack and
/// applies the inverse delta, pushing it onto the redo stack. Redo does the
/// reverse. The stacks have a configurable maximum depth (default 10,000).
///
/// ## Concurrency
///
/// The buffer is not `Send` or `Sync` by default. It is intended to be
/// owned by a single thread (the editor state). For thread-safe access,
/// wrap in an `Arc<RwLock<Buffer>>`.
#[derive(Debug, Clone)]
pub struct Buffer {
    rope: Rope,
    undo_stack: Vec<Delta>,
    redo_stack: Vec<Delta>,
    max_undo_depth: usize,
    /// The "save point" index in the undo stack.
    /// When equal to `undo_stack.len()`, the buffer is in a "clean" state
    /// (no unsaved changes).
    saved_at_index: usize,
    line_cache: CachedLineOffsets,
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer::new()
    }
}

impl Buffer {
    /// Create a new empty buffer.
    pub fn new() -> Self {
        Buffer {
            rope: Rope::new(),
            undo_stack: Vec::with_capacity(1024),
            redo_stack: Vec::with_capacity(128),
            max_undo_depth: 10_000,
            saved_at_index: 0,
            line_cache: CachedLineOffsets::new(),
        }
    }

    /// Create a buffer from a string.
    pub fn from_text(text: &str) -> Self {
        let rope = Rope::from_str(text);
        let mut buf = Buffer {
            rope,
            undo_stack: Vec::with_capacity(1024),
            redo_stack: Vec::with_capacity(128),
            max_undo_depth: 10_000,
            saved_at_index: 0,
            line_cache: CachedLineOffsets::new(),
        };
        buf.line_cache.invalidate();
        buf.line_cache.rebuild(&buf.rope);
        buf
    }

    // ------------------------------------------------------------------
    // Text read operations
    // ------------------------------------------------------------------

    /// Return the full text content.
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    /// Return a slice of the buffer content as a string.
    pub fn slice(&self, start: usize, end: usize) -> EditorResult<String> {
        let len = self.rope.len_bytes();
        if start > len || end > len || start > end {
            return Err(EditorError::DeleteOutOfBounds(start, end, len));
        }
        let char_start = self.rope.byte_to_char(start);
        let char_end = self.rope.byte_to_char(end);
        Ok(self.rope.slice(char_start..char_end).to_string())
    }

    /// Return the total number of bytes in the buffer.
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Return the number of characters in the buffer.
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Return true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    /// Return the number of lines in the buffer.
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Convert a byte offset to (line, column).
    ///
    /// Uses the line cache for O(1) line lookup when it's up-to-date,
    /// otherwise falls back to ropey's O(log n) method.
    pub fn byte_to_line_col(&self, byte_idx: usize) -> EditorResult<(usize, usize)> {
        let len = self.rope.len_bytes();
        if byte_idx > len {
            return Err(EditorError::CursorOutOfBounds(byte_idx, len));
        }
        // ropey's char_idx_to_line is O(log n) — acceptable for now
        let char_idx = self.rope.byte_to_char(byte_idx);
        let line = self.rope.char_to_line(char_idx);
        let line_start_char = self.rope.line_to_char(line);
        let col = char_idx - line_start_char;
        Ok((line, col))
    }

    /// Convert (line, column) to a byte offset.
    pub fn line_col_to_byte(&self, line: usize, col: usize) -> EditorResult<usize> {
        let n_lines = self.rope.len_lines();
        if line >= n_lines {
            return Err(EditorError::LineOutOfBounds(line, n_lines));
        }
        let line_char_start = self.rope.line_to_char(line);
        let line_char_end = self.rope.line_to_char(line + 1);
        let line_len = line_char_end - line_char_start;
        if col > line_len {
            return Err(EditorError::ColumnOutOfBounds(col, line, line_len));
        }
        let char_idx = line_char_start + col;
        Ok(self.rope.char_to_byte(char_idx))
    }

    /// Get the text of a specific line (0-based).
    pub fn get_line(&self, idx: usize) -> EditorResult<String> {
        let n = self.rope.len_lines();
        if idx >= n {
            return Err(EditorError::LineOutOfBounds(idx, n));
        }
        let start_char = self.rope.line_to_char(idx);
        let end_char = self.rope.line_to_char(idx + 1);
        Ok(self.rope.slice(start_char..end_char).to_string())
    }

    /// Get the length of a specific line in characters.
    pub fn line_len_chars(&self, idx: usize) -> EditorResult<usize> {
        let n = self.rope.len_lines();
        if idx >= n {
            return Err(EditorError::LineOutOfBounds(idx, n));
        }
        let start = self.rope.line_to_char(idx);
        let end = self.rope.line_to_char(idx + 1);
        Ok(end - start)
    }

    // ------------------------------------------------------------------
    // Text modification operations
    // All return the Delta for undo/redo purposes.
    // ------------------------------------------------------------------

    /// Insert text at a byte position.
    ///
    /// Returns the `Delta` describing the insertion.
    pub fn insert(&mut self, position: usize, text: &str) -> EditorResult<Delta> {
        let len = self.rope.len_bytes();
        if position > len {
            return Err(EditorError::InsertOutOfBounds(position, len));
        }
        if text.is_empty() {
            return Ok(Delta::new(position, String::new(), String::new()));
        }

        let char_idx = self.rope.byte_to_char(position);
        self.rope.insert(char_idx, text);
        self.line_cache.invalidate();
        self.redo_stack.clear();

        let delta = Delta::new(position, String::new(), text.to_string());
        self.push_undo(delta.clone());
        Ok(delta)
    }

    /// Delete text in a byte range `[start, end)`.
    ///
    /// Returns the `Delta` describing the deletion.
    pub fn delete(&mut self, start: usize, end: usize) -> EditorResult<Delta> {
        let len = self.rope.len_bytes();
        if start > end || end > len {
            return Err(EditorError::DeleteOutOfBounds(start, end, len));
        }
        if start == end {
            return Ok(Delta::new(start, String::new(), String::new()));
        }

        let char_start = self.rope.byte_to_char(start);
        let char_end = self.rope.byte_to_char(end);
        let deleted = self.rope.slice(char_start..char_end).to_string();
        self.rope.remove(char_start..char_end);
        self.line_cache.invalidate();
        self.redo_stack.clear();

        let delta = Delta::new(start, deleted, String::new());
        self.push_undo(delta.clone());
        Ok(delta)
    }

    /// Replace text in a byte range `[start, end)` with new text.
    ///
    /// This is the most common edit operation (e.g., typing replaces a
    /// selection). It is implemented as a single delta for atomic undo.
    pub fn replace(&mut self, start: usize, end: usize, text: &str) -> EditorResult<Delta> {
        let len = self.rope.len_bytes();
        if start > end || end > len {
            return Err(EditorError::DeleteOutOfBounds(start, end, len));
        }

        let char_start = self.rope.byte_to_char(start);
        let char_end = self.rope.byte_to_char(end);

        let deleted = if start < end {
            self.rope.slice(char_start..char_end).to_string()
        } else {
            String::new()
        };

        // Apply: remove old range, insert new text
        self.rope.remove(char_start..char_end);
        self.rope.insert(char_start, text);
        self.line_cache.invalidate();
        self.redo_stack.clear();

        let delta = Delta::new(start, deleted, text.to_string());
        self.push_undo(delta.clone());
        Ok(delta)
    }

    // ------------------------------------------------------------------
    // Undo / Redo
    // ------------------------------------------------------------------

    /// Undo the last edit. Returns the reverted delta.
    pub fn undo(&mut self) -> EditorResult<Delta> {
        let delta = self.undo_stack.pop().ok_or(EditorError::NothingToUndo)?;
        let inverse = delta.invert();

        // Apply the inverse
        self.apply_delta(&inverse)?;
        self.line_cache.invalidate();
        self.redo_stack.push(inverse);
        Ok(delta)
    }

    /// Redo the last undone edit. Returns the re-applied delta.
    pub fn redo(&mut self) -> EditorResult<Delta> {
        let delta = self.redo_stack.pop().ok_or(EditorError::NothingToRedo)?;
        let inverse = delta.invert();

        self.apply_delta(&inverse)?;
        self.line_cache.invalidate();
        self.undo_stack.push(inverse);
        Ok(delta)
    }

    /// Mark the current buffer state as "saved" (no unsaved changes).
    pub fn mark_saved(&mut self) {
        self.saved_at_index = self.undo_stack.len();
    }

    /// Returns `true` if the buffer has no unsaved changes.
    pub fn is_saved(&self) -> bool {
        self.undo_stack.len() == self.saved_at_index
    }

    /// Get the maximum undo depth.
    pub fn max_undo_depth(&self) -> usize {
        self.max_undo_depth
    }

    /// Set the maximum undo depth.
    pub fn set_max_undo_depth(&mut self, depth: usize) {
        self.max_undo_depth = depth;
        self.trim_undo_stack();
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    fn push_undo(&mut self, delta: Delta) {
        if delta.is_noop() {
            return;
        }
        self.undo_stack.push(delta);
        self.trim_undo_stack();
    }

    fn trim_undo_stack(&mut self) {
        if self.undo_stack.len() > self.max_undo_depth {
            let excess = self.undo_stack.len() - self.max_undo_depth;
            self.undo_stack.drain(0..excess);
            // Adjust saved_at_index
            if self.saved_at_index >= excess {
                self.saved_at_index -= excess;
            } else {
                self.saved_at_index = 0;
            }
        }
    }

    fn apply_delta(&mut self, delta: &Delta) -> EditorResult<()> {
        let pos = delta.position;
        let end = pos + delta.deleted.len();

        // Remove the "deleted" text (which is what was inserted)
        if !delta.deleted.is_empty() {
            let len = self.rope.len_bytes();
            if end > len {
                return Err(EditorError::DeleteOutOfBounds(pos, end, len));
            }
            let char_start = self.rope.byte_to_char(pos);
            let char_end = self.rope.byte_to_char(end);
            self.rope.remove(char_start..char_end);
        }

        // Insert the "inserted" text (which is what was originally there)
        if !delta.inserted.is_empty() {
            let len = self.rope.len_bytes();
            if pos > len {
                return Err(EditorError::InsertOutOfBounds(pos, len));
            }
            let char_idx = self.rope.byte_to_char(pos);
            self.rope.insert(char_idx, &delta.inserted);
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let buf = Buffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.len_bytes(), 0);
        assert_eq!(buf.len_lines(), 1); // Rope always has at least 1 line
    }

    #[test]
    fn test_from_text() {
        let buf = Buffer::from_text("hello\nworld");
        assert_eq!(buf.text(), "hello\nworld");
        assert_eq!(buf.len_lines(), 2);
    }

    #[test]
    fn test_insert() {
        let mut buf = Buffer::from_text("hello");
        buf.insert(5, " world").unwrap();
        assert_eq!(buf.text(), "hello world");
    }

    #[test]
    fn test_delete() {
        let mut buf = Buffer::from_text("hello world");
        buf.delete(5, 11).unwrap();
        assert_eq!(buf.text(), "hello");
    }

    #[test]
    fn test_replace() {
        let mut buf = Buffer::from_text("hello world");
        buf.replace(6, 11, "there").unwrap();
        assert_eq!(buf.text(), "hello there");
    }

    #[test]
    fn test_undo_insert() {
        let mut buf = Buffer::from_text("ab");
        buf.insert(1, "XX").unwrap();
        assert_eq!(buf.text(), "aXXb");
        buf.undo().unwrap();
        assert_eq!(buf.text(), "ab");
    }

    #[test]
    fn test_undo_delete() {
        let mut buf = Buffer::from_text("hello world");
        buf.delete(5, 11).unwrap();
        assert_eq!(buf.text(), "hello");
        buf.undo().unwrap();
        assert_eq!(buf.text(), "hello world");
    }

    #[test]
    fn test_undo_redo() {
        let mut buf = Buffer::from_text("a");
        buf.insert(1, "b").unwrap();
        buf.insert(2, "c").unwrap();
        assert_eq!(buf.text(), "abc");
        buf.undo().unwrap();
        assert_eq!(buf.text(), "ab");
        buf.undo().unwrap();
        assert_eq!(buf.text(), "a");
        buf.redo().unwrap();
        assert_eq!(buf.text(), "ab");
        buf.redo().unwrap();
        assert_eq!(buf.text(), "abc");
    }

    #[test]
    fn test_undo_nothing() {
        let mut buf = Buffer::new();
        assert!(buf.undo().is_err());
    }

    #[test]
    fn test_redo_nothing() {
        let mut buf = Buffer::new();
        assert!(buf.redo().is_err());
    }

    #[test]
    fn test_replace_undo() {
        let mut buf = Buffer::from_text("hello world");
        buf.replace(6, 11, "there").unwrap();
        assert_eq!(buf.text(), "hello there");
        buf.undo().unwrap();
        assert_eq!(buf.text(), "hello world");
    }

    #[test]
    fn test_byte_to_line_col() {
        let buf = Buffer::from_text("hello\nworld\nfoo");
        let (line, col) = buf.byte_to_line_col(6).unwrap();
        assert_eq!(line, 1);
        assert_eq!(col, 0);
        let (line2, col2) = buf.byte_to_line_col(12).unwrap();
        assert_eq!(line2, 2);
        assert_eq!(col2, 0);
    }

    #[test]
    fn test_line_col_to_byte() {
        let buf = Buffer::from_text("hello\nworld");
        let byte = buf.line_col_to_byte(1, 2).unwrap();
        assert_eq!(byte, 8); // "hello\nwo"
    }

    #[test]
    fn test_get_line() {
        let buf = Buffer::from_text("hello\nworld\nfoo");
        assert_eq!(buf.get_line(0).unwrap(), "hello\n");
        assert_eq!(buf.get_line(1).unwrap(), "world\n");
        assert_eq!(buf.get_line(2).unwrap(), "foo");
    }

    #[test]
    fn test_line_len_chars() {
        let buf = Buffer::from_text("hello\nworld");
        assert_eq!(buf.line_len_chars(0).unwrap(), 6); // "hello\n"
        assert_eq!(buf.line_len_chars(1).unwrap(), 5); // "world" (no trailing newline)
    }

    #[test]
    fn test_saved_state() {
        let mut buf = Buffer::from_text("hello");
        assert!(buf.is_saved());
        buf.insert(5, " world").unwrap();
        assert!(!buf.is_saved());
        buf.mark_saved();
        assert!(buf.is_saved());
        buf.undo().unwrap();
        assert!(!buf.is_saved());
    }

    #[test]
    fn test_undo_max_depth() {
        let mut buf = Buffer::new();
        buf.set_max_undo_depth(3);
        for i in 0..5 {
            buf.insert(buf.len_bytes(), &format!("{}", i)).unwrap();
        }
        assert_eq!(buf.undo_stack.len(), 3);
        // Undo should still work with remaining entries
        assert!(buf.undo().is_ok());
        assert!(buf.undo().is_ok());
        assert!(buf.undo().is_ok());
        assert!(buf.undo().is_err());
    }

    #[test]
    fn test_insert_out_of_bounds() {
        let mut buf = Buffer::from_text("hi");
        assert!(buf.insert(10, "!").is_err());
    }

    #[test]
    fn test_delete_out_of_bounds() {
        let mut buf = Buffer::from_text("hi");
        assert!(buf.delete(0, 10).is_err());
    }

    #[test]
    fn test_redo_cleared_on_new_edit() {
        let mut buf = Buffer::from_text("a");
        buf.insert(1, "b").unwrap();
        buf.undo().unwrap();
        assert!(buf.redo().is_ok()); // still works
        buf.insert(1, "c").unwrap();
        assert!(buf.redo().is_err()); // redo cleared by new edit
    }
}
