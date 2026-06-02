//! Error types for the editor subsystem.

use thiserror::Error;

/// Errors that can occur during editor operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum EditorError {
    /// Attempted to move cursor past the end of the buffer
    #[error("cursor position out of bounds: {0} > {1}")]
    CursorOutOfBounds(usize, usize),

    /// Invalid selection range (start > end)
    #[error("invalid selection range: start {0} > end {1}")]
    InvalidSelection(usize, usize),

    /// Undo stack is empty
    #[error("nothing to undo")]
    NothingToUndo,

    /// Redo stack is empty
    #[error("nothing to redo")]
    NothingToRedo,

    /// Undo depth limit reached
    #[error("undo depth limit reached")]
    UndoDepthLimit,

    /// Insert position out of bounds
    #[error("insert position {0} out of bounds (len {1})")]
    InsertOutOfBounds(usize, usize),

    /// Delete range out of bounds
    #[error("delete range {0}..{1} out of bounds (len {2})")]
    DeleteOutOfBounds(usize, usize, usize),

    /// Line index out of bounds
    #[error("line {0} out of bounds (line count {1})")]
    LineOutOfBounds(usize, usize),

    /// Column index out of bounds on the line
    #[error("column {0} out of bounds on line {1} (line len {2})")]
    ColumnOutOfBounds(usize, usize, usize),

    /// General internal error
    #[error("internal error: {0}")]
    Internal(String),
}

/// Convenience alias for `Result<T, EditorError>`.
pub type EditorResult<T> = Result<T, EditorError>;
