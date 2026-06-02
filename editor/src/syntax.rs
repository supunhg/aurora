//! Syntax highlighting pipeline using tree-sitter.
//!
//! The highlight pipeline works in three stages:
//!
//! 1. **Parse** — Run the tree-sitter parser on the visible lines
//! 2. **Query** — Apply highlight queries to extract scope → token mappings
//! 3. **Style** — Map scopes to colors/themes and produce `HighlightRange` batches
//!
//! ## Design
//!
//! - Parsing runs off the UI thread (background worker)
//! - Results are snapshotted and sent to the UI thread via `Arc<Vec<HighlightRange>>`
//! - Only the visible range (+ buffer) is re-parsed on edits
//!
//! ## Feature Gate
//!
//! This module is only available with the `treesitter` feature enabled.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Highlight data structures
// ---------------------------------------------------------------------------

/// A range of text with a specific highlight style.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HighlightRange {
    /// Start byte offset in the buffer.
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// The highlight scope name (e.g., "keyword", "string", "comment").
    pub scope: String,
}

/// A snapshot of the syntax highlight state for the editor.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HighlightSnapshot {
    /// All highlight ranges, sorted by start byte offset.
    pub ranges: Vec<HighlightRange>,
    /// The version of the buffer that was highlighted.
    pub buffer_version: u64,
}

// ---------------------------------------------------------------------------
// Theme / scope mapping
// ---------------------------------------------------------------------------

/// A named color in the highlight theme.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl ThemeColor {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        ThemeColor { r, g, b, a }
    }
}

/// A mapping from scope name to display color.
pub type ScopeTheme = Vec<(String, ThemeColor)>;

/// The default dark theme for editor syntax highlighting.
pub fn default_dark_theme() -> ScopeTheme {
    vec![
        ("comment".to_string(), ThemeColor::new(98, 114, 164, 255)),
        ("keyword".to_string(), ThemeColor::new(189, 147, 249, 255)),
        ("string".to_string(), ThemeColor::new(80, 250, 123, 255)),
        ("number".to_string(), ThemeColor::new(255, 184, 108, 255)),
        ("function".to_string(), ThemeColor::new(80, 250, 123, 255)),
        ("type".to_string(), ThemeColor::new(139, 233, 253, 255)),
        ("variable".to_string(), ThemeColor::new(248, 248, 242, 255)),
        ("constant".to_string(), ThemeColor::new(255, 184, 108, 255)),
        ("operator".to_string(), ThemeColor::new(255, 121, 198, 255)),
        ("builtin".to_string(), ThemeColor::new(139, 233, 253, 255)),
    ]
}

/// The default light theme.
pub fn default_light_theme() -> ScopeTheme {
    vec![
        ("comment".to_string(), ThemeColor::new(148, 158, 177, 255)),
        ("keyword".to_string(), ThemeColor::new(86, 61, 214, 255)),
        ("string".to_string(), ThemeColor::new(3, 152, 65, 255)),
        ("number".to_string(), ThemeColor::new(217, 95, 2, 255)),
        ("function".to_string(), ThemeColor::new(0, 64, 128, 255)),
        ("type".to_string(), ThemeColor::new(0, 102, 153, 255)),
        ("variable".to_string(), ThemeColor::new(34, 34, 34, 255)),
        ("constant".to_string(), ThemeColor::new(217, 95, 2, 255)),
        ("operator".to_string(), ThemeColor::new(160, 50, 120, 255)),
        ("builtin".to_string(), ThemeColor::new(0, 102, 153, 255)),
    ]
}

// ---------------------------------------------------------------------------
// HighlightEngine (tree-sitter)
// ---------------------------------------------------------------------------

/// The highlight engine, wrapping tree-sitter parsers and highlight queries.
///
/// Only available when the `treesitter` feature is enabled.
#[cfg(feature = "treesitter")]
pub struct HighlightEngine {
    /// The tree-sitter language (e.g., tree_sitter_rust::language()).
    language: tree_sitter::Language,
    /// The compiled highlight query.
    highlights_query: tree_sitter::Query,
    /// Reusable parser instance.
    parser: tree_sitter::Parser,
}

#[cfg(feature = "treesitter")]
impl HighlightEngine {
    /// Create a new highlight engine for the given language.
    ///
    /// The `highlights_query_str` should be the raw tree-sitter query text
    /// for the language's highlights (usually from `highlights.scm`).
    pub fn new(
        language: tree_sitter::Language,
        highlights_query_str: &str,
    ) -> Result<Self, String> {
        let query = tree_sitter::Query::new(&language, highlights_query_str)
            .map_err(|e| format!("failed to compile query: {}", e))?;

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| format!("failed to set language: {}", e))?;

        Ok(HighlightEngine {
            language,
            highlights_query: query,
            parser,
        })
    }

    /// Parse and highlight the given source text.
    ///
    /// Returns a list of `(start_byte, end_byte, scope_name)` ranges.
    pub fn highlight(&mut self, source: &str) -> Vec<HighlightRange> {
        let tree = match self.parser.parse(source, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let root_node = tree.root_node();
        let mut cursor = tree_sitter::QueryCursor::new();
        let matches = cursor.matches(&self.highlights_query, root_node, source.as_bytes());
        let capture_names = self.highlights_query.capture_names();

        let mut ranges: Vec<HighlightRange> = Vec::new();

        for match_ in matches {
            for capture in match_.captures {
                let node = capture.node;
                let start = node.byte_range().start;
                let end = node.byte_range().end;
                let capture_name = capture_names[capture.index as usize].to_string();

                ranges.push(HighlightRange {
                    start,
                    end,
                    scope: capture_name,
                });
            }
        }

        // Sort by start position
        ranges.sort_by_key(|r| r.start);
        ranges
    }
}

// ---------------------------------------------------------------------------
// ScopeClassifier — rule-based highlighting (no tree-sitter)
// ---------------------------------------------------------------------------

/// A simple rule-based scope classifier that works without tree-sitter.
///
/// This provides basic syntax highlighting for common languages when the
/// `treesitter` feature is disabled. It's fast but limited — tree-sitter
/// is the recommended path for full accuracy.
pub struct ScopeClassifier;

impl ScopeClassifier {
    /// Classify a line of code into highlight ranges.
    ///
    /// This uses simple regex-like rules to identify:
    /// - Line comments (`//`, `#`, `;`)
    /// - Strings (single/double quoted)
    /// - Numbers (integers and floats)
    /// - Keywords (language-specific)
    ///
    /// `line_start_byte` is the byte offset of the start of this line in the buffer.
    pub fn classify_line(
        line: &str,
        line_start_byte: usize,
        keywords: &[&str],
    ) -> Vec<HighlightRange> {
        let mut ranges = Vec::new();
        let bytes = line.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            // Skip whitespace
            if bytes[i].is_ascii_whitespace() {
                i += 1;
                continue;
            }

            // Line comment: //
            if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                ranges.push(HighlightRange {
                    start: line_start_byte + i,
                    end: line_start_byte + len,
                    scope: "comment".to_string(),
                });
                break;
            }

            // Line comment: # (Python, Bash, etc.)
            if bytes[i] == b'#' {
                // Check that it's not part of an expression (heuristic: preceded by whitespace or start)
                if i == 0 || bytes[i - 1].is_ascii_whitespace() {
                    ranges.push(HighlightRange {
                        start: line_start_byte + i,
                        end: line_start_byte + len,
                        scope: "comment".to_string(),
                    });
                    break;
                }
            }

            // String: double quoted
            if bytes[i] == b'"' {
                let start = i;
                i += 1;
                while i < len && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1; // skip escaped char
                    }
                    i += 1;
                }
                if i < len {
                    i += 1; // closing quote
                }
                ranges.push(HighlightRange {
                    start: line_start_byte + start,
                    end: line_start_byte + i,
                    scope: "string".to_string(),
                });
                continue;
            }

            // String: single quoted
            if bytes[i] == b'\'' {
                let start = i;
                i += 1;
                while i < len && bytes[i] != b'\'' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                ranges.push(HighlightRange {
                    start: line_start_byte + start,
                    end: line_start_byte + i,
                    scope: "string".to_string(),
                });
                continue;
            }

            // Number: digits (including decimal)
            if bytes[i].is_ascii_digit() {
                let start = i;
                i += 1;
                while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                ranges.push(HighlightRange {
                    start: line_start_byte + start,
                    end: line_start_byte + i,
                    scope: "number".to_string(),
                });
                continue;
            }

            // Identifier or keyword
            if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
                let start = i;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &line[start..i];

                if keywords.contains(&word) {
                    ranges.push(HighlightRange {
                        start: line_start_byte + start,
                        end: line_start_byte + i,
                        scope: "keyword".to_string(),
                    });
                }
                continue;
            }

            // Operators and punctuation
            if bytes[i] == b'{'
                || bytes[i] == b'}'
                || bytes[i] == b'('
                || bytes[i] == b')'
                || bytes[i] == b'['
                || bytes[i] == b']'
                || bytes[i] == b';'
                || bytes[i] == b':'
                || bytes[i] == b','
                || bytes[i] == b'.'
            {
                i += 1;
                continue;
            }

            // Other operators (skip single char)
            if bytes[i].is_ascii_punctuation() {
                i += 1;
                continue;
            }

            i += 1;
        }

        ranges
    }
}

/// Common keywords for Rust.
pub const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "union",
    "unsafe", "use", "where", "while", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
];

/// Common keywords for TypeScript/JavaScript.
pub const TYPESCRIPT_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "as",
    "any",
    "boolean",
    "number",
    "string",
    "interface",
    "type",
    "module",
    "namespace",
    "abstract",
    "implements",
    "private",
    "protected",
    "public",
    "readonly",
    "declare",
];

/// Common keywords for Python.
pub const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_rust_comment() {
        let line = "// this is a comment";
        let ranges = ScopeClassifier::classify_line(line, 0, RUST_KEYWORDS);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].scope, "comment");
    }

    #[test]
    fn test_classify_string() {
        let line = r#"let x = "hello world";"#;
        let ranges = ScopeClassifier::classify_line(line, 0, RUST_KEYWORDS);
        let strings: Vec<_> = ranges.iter().filter(|r| r.scope == "string").collect();
        assert_eq!(strings.len(), 1);
        assert_eq!(&line[strings[0].start..strings[0].end], r#""hello world""#);
    }

    #[test]
    fn test_classify_keywords() {
        let line = "fn main() {";
        let ranges = ScopeClassifier::classify_line(line, 0, RUST_KEYWORDS);
        let kw: Vec<_> = ranges.iter().filter(|r| r.scope == "keyword").collect();
        assert_eq!(kw.len(), 1);
        assert_eq!(&line[kw[0].start..kw[0].end], "fn");
    }

    #[test]
    fn test_classify_number() {
        let line = "let x = 42;";
        let ranges = ScopeClassifier::classify_line(line, 0, RUST_KEYWORDS);
        let numbers: Vec<_> = ranges.iter().filter(|r| r.scope == "number").collect();
        assert_eq!(numbers.len(), 1);
        assert_eq!(&line[numbers[0].start..numbers[0].end], "42");
    }

    #[test]
    fn test_default_themes_exist() {
        let dark = default_dark_theme();
        assert!(!dark.is_empty());
        let light = default_light_theme();
        assert!(!light.is_empty());
    }
}
