//! Cursor management for the editor.
//!
//! A cursor represents a logical insertion point in the buffer, stored as
//! a byte offset. Multiple cursors are supported for multi-cursor editing.
//!
//! Each cursor can also hold an optional selection range.

use crate::error::{EditorError, EditorResult};
use serde::{Deserialize, Serialize};

/// A single cursor with an optional selection.
///
/// The cursor position is always stored as a byte offset into the buffer.
/// The selection is a half-open byte range `[anchor, head)` where `anchor`
/// is the fixed end and `head` is the moving end. When `anchor == head`,
/// there is no selection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cursor {
    /// The main cursor position (byte offset).
    pub position: usize,
    /// The selection anchor (byte offset). Equal to `position` when no selection.
    pub anchor: usize,
    /// The selection head (byte offset). Equal to `position` when no selection.
    pub head: usize,
    /// The "preferred" column for vertical navigation (up/down arrows).
    /// `None` means use the current column.
    pub preferred_col: Option<usize>,
}

impl Cursor {
    /// Create a new cursor at the given byte position.
    pub fn new(position: usize) -> Self {
        Cursor {
            position,
            anchor: position,
            head: position,
            preferred_col: None,
        }
    }

    /// Set the cursor position and clear the selection.
    pub fn set_position(&mut self, pos: usize) {
        self.position = pos;
        self.anchor = pos;
        self.head = pos;
    }

    /// Returns `true` if there is no active selection.
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }

    /// Returns the selection range as `(start, end)` where `start <= end`.
    pub fn selection_range(&self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// Returns the direction of the selection: `true` if head >= anchor.
    pub fn selection_forward(&self) -> bool {
        self.head >= self.anchor
    }

    /// Start a selection at the current position.
    /// Moves the head to `new_head` while keeping the anchor at `position`.
    pub fn start_selection(&mut self, new_head: usize) {
        self.anchor = self.position;
        self.head = new_head;
    }

    /// Extend the selection to a new head position.
    pub fn extend_selection(&mut self, new_head: usize) {
        self.head = new_head;
    }

    /// Collapse the selection, moving to one end.
    /// If `forward` is true, collapses to the head; otherwise to the anchor.
    pub fn collapse_selection(&mut self, forward: bool) {
        if forward {
            self.position = self.head;
        } else {
            self.position = self.anchor;
        }
        self.anchor = self.position;
        self.head = self.position;
    }
}

/// A set of cursors for multi-cursor editing.
///
/// Cursors are stored in order of their byte position. Operations that
/// modify all cursors (like inserting text at each cursor) must process
/// cursors in reverse order to maintain correct positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorSet {
    cursors: Vec<Cursor>,
    /// Index of the primary cursor (the one that was most recently used).
    primary: usize,
}

impl CursorSet {
    /// Create a new cursor set with a single cursor at position 0.
    pub fn new() -> Self {
        CursorSet {
            cursors: vec![Cursor::new(0)],
            primary: 0,
        }
    }

    /// Create a new cursor set with a single cursor at the given position.
    pub fn at_position(pos: usize) -> Self {
        CursorSet {
            cursors: vec![Cursor::new(pos)],
            primary: 0,
        }
    }

    /// Get a reference to the primary cursor.
    pub fn primary(&self) -> &Cursor {
        &self.cursors[self.primary]
    }

    /// Get a mutable reference to the primary cursor.
    pub fn primary_mut(&mut self) -> &mut Cursor {
        &mut self.cursors[self.primary]
    }

    /// Get a reference to all cursors.
    pub fn all(&self) -> &[Cursor] {
        &self.cursors
    }

    /// Get a mutable reference to all cursors.
    pub fn all_mut(&mut self) -> &mut [Cursor] {
        &mut self.cursors
    }

    /// Get an iterator over all cursors.
    pub fn iter(&self) -> impl Iterator<Item = &Cursor> {
        self.cursors.iter()
    }

    /// Get a mutable iterator over all cursors.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Cursor> {
        self.cursors.iter_mut()
    }

    /// Return the number of cursors.
    pub fn len(&self) -> usize {
        self.cursors.len()
    }

    /// Returns `true` if the cursor set is empty.
    pub fn is_empty(&self) -> bool {
        self.cursors.is_empty()
    }

    /// Add a new cursor at the given position.
    /// Returns the index of the new cursor, which becomes the primary.
    pub fn add_cursor(&mut self, position: usize) -> usize {
        // Insert in sorted position order
        let idx = match self.cursors.binary_search_by(|c| c.position.cmp(&position)) {
            Ok(i) => i,
            Err(i) => i,
        };
        let new_cursor = Cursor::new(position);
        self.cursors.insert(idx, new_cursor);
        self.primary = idx;
        idx
    }

    /// Remove all cursors except the primary, and collapse its selection.
    pub fn collapse_to_primary(&mut self) {
        let primary_pos = self.cursors[self.primary].position;
        self.cursors.clear();
        self.cursors.push(Cursor::new(primary_pos));
        self.primary = 0;
    }

    /// Remove the cursor at the given index. If it's the last cursor,
    /// a new cursor is created at position 0.
    pub fn remove_cursor(&mut self, idx: usize) {
        if self.cursors.len() <= 1 {
            // Don't remove the last cursor; just reset it
            self.cursors[0].set_position(0);
            self.primary = 0;
            return;
        }
        self.cursors.remove(idx);
        if self.primary >= self.cursors.len() {
            self.primary = self.cursors.len() - 1;
        }
    }

    /// Move all cursors by a delta offset (used after text insertion/deletion).
    /// The offset is applied to all cursors at or after `from_position`.
    pub fn apply_delta_offset(&mut self, from_position: usize, delta: isize) {
        for cursor in &mut self.cursors {
            if cursor.position >= from_position {
                cursor.position = (cursor.position as isize + delta).max(0) as usize;
            }
            if cursor.anchor >= from_position {
                cursor.anchor = (cursor.anchor as isize + delta).max(0) as usize;
            }
            if cursor.head >= from_position {
                cursor.head = (cursor.head as isize + delta).max(0) as usize;
            }
        }
    }

    /// Validate that all cursor positions are within bounds.
    pub fn validate(&self, buffer_len: usize) -> EditorResult<()> {
        for cursor in self.cursors.iter() {
            if cursor.position > buffer_len {
                return Err(EditorError::CursorOutOfBounds(cursor.position, buffer_len));
            }
            if cursor.anchor > buffer_len || cursor.head > buffer_len {
                return Err(EditorError::CursorOutOfBounds(
                    cursor.anchor.max(cursor.head),
                    buffer_len,
                ));
            }
        }
        Ok(())
    }
}

impl Default for CursorSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_new() {
        let c = Cursor::new(5);
        assert_eq!(c.position, 5);
        assert!(c.is_collapsed());
    }

    #[test]
    fn test_cursor_selection() {
        let mut c = Cursor::new(10);
        c.start_selection(20);
        assert!(!c.is_collapsed());
        assert_eq!(c.selection_range(), (10, 20));
        assert!(c.selection_forward());
    }

    #[test]
    fn test_cursor_selection_backward() {
        let mut c = Cursor::new(10);
        c.start_selection(5);
        assert_eq!(c.selection_range(), (5, 10));
        assert!(!c.selection_forward());
    }

    #[test]
    fn test_cursor_collapse_selection() {
        let mut c = Cursor::new(10);
        c.start_selection(20);
        c.collapse_selection(false); // collapse to anchor
        assert_eq!(c.position, 10);
        assert!(c.is_collapsed());
    }

    #[test]
    fn test_cursor_set_new() {
        let cs = CursorSet::new();
        assert_eq!(cs.len(), 1);
        assert_eq!(cs.primary().position, 0);
    }

    #[test]
    fn test_add_remove_cursor() {
        let mut cs = CursorSet::at_position(0);
        cs.add_cursor(10);
        assert_eq!(cs.len(), 2);
        cs.remove_cursor(1);
        assert_eq!(cs.len(), 1);
    }

    #[test]
    fn test_cursor_delta_offset() {
        let mut cs = CursorSet::at_position(5);
        cs.add_cursor(10);
        cs.add_cursor(20);
        // Insert 3 chars at position 8 => shift all cursors >= 8 by +3
        cs.apply_delta_offset(8, 3);
        assert_eq!(cs.all()[0].position, 5); // unchanged
        assert_eq!(cs.all()[1].position, 13); // 10 + 3
        assert_eq!(cs.all()[2].position, 23); // 20 + 3
    }

    #[test]
    fn test_validate() {
        let cs = CursorSet::at_position(5);
        assert!(cs.validate(10).is_ok());
        assert!(cs.validate(3).is_err());
    }
}
