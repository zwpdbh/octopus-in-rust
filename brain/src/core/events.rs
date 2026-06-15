use serde_json::Value;

/// An event emitted by a Brain turn.
#[derive(Debug, Clone)]
pub enum BrainEvent {
    /// Start of a turn.
    TurnBegin,

    /// A text fragment produced by the LLM.
    TextPart(String),

    /// A thinking/reasoning fragment produced by the LLM.
    ThinkingPart(String),

    /// The LLM requested a tool call.
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },

    /// A tool call requires user/policy approval before executing.
    ApprovalRequested {
        tool_call_id: String,
        tool_name: String,
        arguments: Value,
    },

    /// Approval decision for a tool call.
    ApprovalResolved {
        tool_call_id: String,
        approved: bool,
        reason: Option<String>,
    },

    /// A tool call completed with a result.
    ToolResult {
        id: String,
        output: String,
        is_error: bool,
    },

    /// End of a turn.
    TurnEnd,

    /// An error occurred during the turn.
    Error(String),
}
