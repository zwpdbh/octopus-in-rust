//! Streaming event types for the agent chat wire protocol.

use serde::Deserialize;

/// Event streamed by an agent chat SSE endpoint, parsed from each `data:` line.
///
/// This is the client-side mirror of `QaStreamEvent` in
/// `apps/fafcn-server/src/handlers/qa.rs`. The two must stay wire-compatible
/// (same `kind` tag and field names); the server side is the source of truth.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind")]
pub enum AgentStreamEvent {
    /// Incremental chunk of the assistant's visible answer.
    TextDelta { delta: String },
    /// Incremental chunk of the assistant's reasoning.
    ThinkingDelta { delta: String },
    /// The agent invoked a tool.
    ToolCall {
        name: String,
        #[allow(dead_code)]
        arguments: serde_json::Value,
    },
    /// A tool invocation finished.
    ToolResult {
        output: String,
        #[allow(dead_code)]
        is_error: bool,
    },
    /// The turn finished; no more events follow.
    Done,
}
