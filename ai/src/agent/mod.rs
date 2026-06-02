use crate::agent::tools::{Tool, ToolResult};
use crate::freellm::{ChatMessage, FreeLlmClient};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, warn};

pub mod tools;

/// A step in the agent's execution trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStep {
    /// The AI's reasoning/thought.
    Thought { content: String },
    /// A tool call the AI decided to make.
    ToolCall {
        tool_name: String,
        arguments: serde_json::Value,
    },
    /// The result of a tool execution.
    ToolResult {
        tool_name: String,
        output: String,
        success: bool,
    },
    /// A proposed file change awaiting approval.
    FileChange {
        path: String,
        old_content: Option<String>,
        new_content: String,
    },
    /// The final answer from the agent.
    FinalAnswer { content: String },
}

/// Status of an agent task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Running,
    WaitingForApproval,
    Completed,
    Failed(String),
}

/// The agent loop that orchestrates AI + tools.
pub struct AgentLoop {
    client: Arc<FreeLlmClient>,
    model: String,
    tools: Vec<Arc<dyn Tool>>,
    max_iterations: usize,
    system_prompt: String,
}

/// Configuration for an agent task.
pub struct AgentTask {
    pub user_request: String,
    pub context: Option<String>,
}

impl AgentLoop {
    /// Create a new agent loop.
    pub fn new(client: Arc<FreeLlmClient>, model: &str) -> Self {
        Self {
            client,
            model: model.to_string(),
            tools: Vec::new(),
            max_iterations: 20,
            system_prompt: Self::default_system_prompt(),
        }
    }

    /// Register a tool the agent can use.
    pub fn register_tool(&mut self, tool: Arc<dyn Tool>) {
        self.tools.push(tool);
    }

    /// Set the maximum number of tool-call iterations.
    pub fn max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    /// Set a custom system prompt.
    pub fn system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = prompt.to_string();
        self
    }

    /// Get the list of tools as OpenAI function definitions.
    fn tool_definitions(&self) -> Vec<serde_json::Value> {
        self.tools.iter().map(|t| t.to_openai_tool()).collect()
    }

    /// Run the agent loop for a given task.
    ///
    /// Returns the execution trace (list of steps) and any proposed file changes.
    pub async fn run(&self, task: AgentTask) -> AgentResult {
        let mut messages = vec![ChatMessage {
            role: "system".into(),
            content: self.system_prompt.clone(),
        }];

        if let Some(ref ctx) = task.context {
            messages.push(ChatMessage {
                role: "system".into(),
                content: format!("Additional context:\n{}", ctx),
            });
        }

        messages.push(ChatMessage {
            role: "user".into(),
            content: task.user_request,
        });

        let mut trace = Vec::new();
        let mut proposed_changes = Vec::new();

        for iteration in 0..self.max_iterations {
            debug!(
                "[agent] Iteration {}/{}",
                iteration + 1,
                self.max_iterations
            );

            // Call the LLM
            let response = match self
                .client
                .chat_completion_with_tools(&self.model, messages.clone(), self.tool_definitions())
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!("[agent] LLM call failed: {}", e);
                    return AgentResult {
                        trace,
                        proposed_changes,
                        status: AgentStatus::Failed(format!("LLM error: {}", e)),
                    };
                }
            };

            // Process the response
            let choice = match response.choices.first() {
                Some(c) => c,
                None => {
                    return AgentResult {
                        trace,
                        proposed_changes,
                        status: AgentStatus::Failed("No response from LLM".into()),
                    };
                }
            };

            // Check for tool calls
            if let Some(ref tool_calls) = choice.message.tool_calls {
                for tool_call in tool_calls {
                    let tool_name = &tool_call.function.name;
                    let args: serde_json::Value =
                        serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::json!({}));

                    trace.push(AgentStep::ToolCall {
                        tool_name: tool_name.clone(),
                        arguments: args.clone(),
                    });

                    // Find and execute the tool
                    let result =
                        if let Some(tool) = self.tools.iter().find(|t| t.name() == tool_name) {
                            tool.execute(args).await
                        } else {
                            ToolResult {
                                success: false,
                                output: format!("Unknown tool: {}", tool_name),
                                proposed_content: None,
                                file_path: None,
                            }
                        };

                    // Track proposed changes
                    if let (Some(content), Some(path)) =
                        (&result.proposed_content, &result.file_path)
                    {
                        proposed_changes.push(ProposedChange {
                            path: path.clone(),
                            content: content.clone(),
                            approved: None,
                        });
                        trace.push(AgentStep::FileChange {
                            path: path.to_string_lossy().to_string(),
                            old_content: None,
                            new_content: content.clone(),
                        });
                    }

                    trace.push(AgentStep::ToolResult {
                        tool_name: tool_name.clone(),
                        output: result.output.clone(),
                        success: result.success,
                    });

                    // Add tool result to conversation
                    messages.push(ChatMessage {
                        role: "assistant".into(),
                        content: format!("[Called tool: {}]", tool_name),
                    });
                    messages.push(ChatMessage {
                        role: "tool".into(),
                        content: result.output,
                    });
                }
            } else {
                // No tool calls — the AI is done
                let content = choice.message.content.clone().unwrap_or_default();
                trace.push(AgentStep::FinalAnswer {
                    content: content.clone(),
                });

                return AgentResult {
                    trace,
                    proposed_changes,
                    status: AgentStatus::Completed,
                };
            }
        }

        AgentResult {
            trace,
            proposed_changes,
            status: AgentStatus::Failed(format!(
                "Exceeded max iterations ({})",
                self.max_iterations
            )),
        }
    }

    fn default_system_prompt() -> String {
        "You are Aurora, an AI coding assistant. You help users with software engineering tasks.

When asked to make changes:
1. First, understand the codebase by reading relevant files
2. Plan the changes needed
3. Make the changes using the available tools
4. Verify the changes work

You have access to these tools:
- read_file: Read file contents
- write_file: Write/modify files (changes require user approval)
- search_files: Find files by glob pattern
- list_directory: Browse directory contents
- grep: Search for text across files
- run_command: Execute shell commands

Always explain what you're doing and why. When writing files, show the user what you plan to change."
            .to_string()
    }
}

/// A proposed file change awaiting approval.
#[derive(Debug, Clone)]
pub struct ProposedChange {
    pub path: std::path::PathBuf,
    pub content: String,
    pub approved: Option<bool>,
}

/// The result of an agent run.
#[derive(Debug)]
pub struct AgentResult {
    pub trace: Vec<AgentStep>,
    pub proposed_changes: Vec<ProposedChange>,
    pub status: AgentStatus,
}

impl AgentResult {
    /// Get a human-readable summary of the trace.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        for step in &self.trace {
            match step {
                AgentStep::Thought { content } => {
                    lines.push(format!("💭 {}", content));
                }
                AgentStep::ToolCall {
                    tool_name,
                    arguments,
                } => {
                    lines.push(format!("🔧 Calling {}({})", tool_name, arguments));
                }
                AgentStep::ToolResult {
                    tool_name,
                    output,
                    success,
                } => {
                    let icon = if *success { "✅" } else { "❌" };
                    let preview = if output.len() > 200 {
                        format!("{}...", &output[..200])
                    } else {
                        output.clone()
                    };
                    lines.push(format!("{} {} result: {}", icon, tool_name, preview));
                }
                AgentStep::FileChange { path, .. } => {
                    lines.push(format!("📝 Proposed change to {}", path));
                }
                AgentStep::FinalAnswer { content } => {
                    lines.push(format!("💬 {}", content));
                }
            }
        }
        lines.join("\n")
    }

    /// Get all proposed file changes.
    pub fn changes(&self) -> Vec<&ProposedChange> {
        self.proposed_changes.iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_status() {
        assert_eq!(AgentStatus::Idle, AgentStatus::Idle);
        assert_eq!(AgentStatus::Running, AgentStatus::Running);
        assert_ne!(AgentStatus::Completed, AgentStatus::Failed("test".into()));
    }

    #[test]
    fn test_agent_result_summary() {
        let result = AgentResult {
            trace: vec![
                AgentStep::Thought {
                    content: "I need to read the file first".into(),
                },
                AgentStep::ToolCall {
                    tool_name: "read_file".into(),
                    arguments: serde_json::json!({"path": "/tmp/test.rs"}),
                },
                AgentStep::ToolResult {
                    tool_name: "read_file".into(),
                    output: "fn main() {}".into(),
                    success: true,
                },
                AgentStep::FinalAnswer {
                    content: "The file contains a simple main function".into(),
                },
            ],
            proposed_changes: vec![],
            status: AgentStatus::Completed,
        };

        let summary = result.summary();
        assert!(summary.contains("read_file"));
        assert!(summary.contains("main function"));
        assert_eq!(result.status, AgentStatus::Completed);
    }
}
