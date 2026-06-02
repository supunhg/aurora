//! AI context pruning & token budgeting.
//! Prepares request context by selecting the most relevant file chunks,
//! symbols, and git diff, staying within the token budget.

/// Type of AI request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    InlineCompletion,
    Chat,
    Refactor,
    Agent,
}

/// A chunk of file content with metadata.
#[derive(Debug, Clone)]
pub struct ContextChunk {
    pub path: String,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub relevance_score: f64,
}

/// Reference to a symbol in the workspace.
#[derive(Debug, Clone)]
pub struct SymbolRef {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub line: usize,
}

/// Git diff patch.
#[derive(Debug, Clone)]
pub struct DiffPatch {
    pub files_changed: Vec<String>,
    pub diff_content: String,
}

/// The complete AI context payload.
#[derive(Debug, Clone)]
pub struct AIContext {
    pub file_chunks: Vec<ContextChunk>,
    pub symbols: Vec<SymbolRef>,
    pub git_diff: Option<DiffPatch>,
    pub cursor_line: usize,
    pub cursor_column: usize,
    pub token_budget: usize,
    pub request_type: RequestType,
    pub latency_budget_ms: u64,
    pub conversation_id: Option<String>,
}

impl AIContext {
    /// Estimate the number of tokens in the context.
    /// Rough approximation: ~4 characters per token for most code.
    pub fn estimate_token_count(&self) -> usize {
        let mut total_chars = 0usize;
        for chunk in &self.file_chunks {
            total_chars += chunk.content.len();
        }
        for sym in &self.symbols {
            total_chars += sym.name.len();
        }
        if let Some(ref diff) = self.git_diff {
            total_chars += diff.diff_content.len();
        }
        total_chars / 4
    }

    /// Check if the context fits within the token budget.
    pub fn within_budget(&self) -> bool {
        self.estimate_token_count() <= self.token_budget
    }
}

/// Prunes context to fit within a token budget.
/// Keeps the most relevant chunks and drops low-relevance ones.
pub struct ContextPruner {
    default_token_budget: usize,
}

impl ContextPruner {
    pub fn new(default_token_budget: usize) -> Self {
        Self {
            default_token_budget,
        }
    }

    /// Prune the context to fit within the budget.
    pub fn prune(&self, mut ctx: AIContext, max_tokens: Option<usize>) -> AIContext {
        let budget = max_tokens.unwrap_or(self.default_token_budget);

        // Sort chunks by relevance (highest first)
        ctx.file_chunks.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Keep chunks until we hit the budget
        let mut used = 0usize;
        ctx.file_chunks.retain(|chunk| {
            let chunk_tokens = chunk.content.len() / 4;
            if used + chunk_tokens <= budget {
                used += chunk_tokens;
                true
            } else {
                false
            }
        });

        // Trim symbol list
        let max_symbols = budget / 20;
        if ctx.symbols.len() > max_symbols {
            ctx.symbols.truncate(max_symbols);
        }

        ctx
    }

    /// Build a minimal context for inline completions (fast path).
    pub fn minimal_context(
        &self,
        _current_file: &str,
        cursor_line: usize,
        cursor_column: usize,
        _surrounding_lines: usize,
    ) -> AIContext {
        AIContext {
            file_chunks: Vec::new(), // caller fills this
            symbols: Vec::new(),
            git_diff: None,
            cursor_line,
            cursor_column,
            token_budget: self.default_token_budget,
            request_type: RequestType::InlineCompletion,
            latency_budget_ms: 200,
            conversation_id: None,
        }
    }
}

impl Default for ContextPruner {
    fn default() -> Self {
        Self::new(8192)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_estimation() {
        let ctx = AIContext {
            file_chunks: vec![ContextChunk {
                path: "test.rs".into(),
                content: "fn hello() { println!(\"world\"); }".into(),
                start_line: 0,
                end_line: 1,
                relevance_score: 1.0,
            }],
            symbols: vec![],
            git_diff: None,
            cursor_line: 0,
            cursor_column: 0,
            token_budget: 8192,
            request_type: RequestType::Chat,
            latency_budget_ms: 500,
            conversation_id: None,
        };
        assert!(ctx.estimate_token_count() > 0);
        assert!(ctx.within_budget());
    }

    #[test]
    fn test_pruner_removes_low_relevance() {
        let pruner = ContextPruner::new(100);
        let ctx = AIContext {
            file_chunks: vec![
                ContextChunk {
                    path: "a.rs".into(),
                    content: "a".repeat(200),
                    start_line: 0,
                    end_line: 1,
                    relevance_score: 0.1,
                },
                ContextChunk {
                    path: "b.rs".into(),
                    content: "b".repeat(200),
                    start_line: 0,
                    end_line: 1,
                    relevance_score: 0.9,
                },
            ],
            symbols: vec![],
            git_diff: None,
            cursor_line: 0,
            cursor_column: 0,
            token_budget: 100,
            request_type: RequestType::Chat,
            latency_budget_ms: 500,
            conversation_id: None,
        };

        let pruned = pruner.prune(ctx, Some(60));
        assert_eq!(pruned.file_chunks.len(), 1);
        assert_eq!(pruned.file_chunks[0].relevance_score, 0.9);
    }
}
