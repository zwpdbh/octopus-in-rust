use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::core::errors::BrainErrorCategory;

/// Identifier for a checkpoint.
pub type CheckpointId = usize;

/// Sender used by frontends to provide a refreshed provider.
///
/// Wrapped so that [`BrainEvent`] can remain `Debug + Clone`.
#[derive(Clone)]
pub struct ProviderRefreshSender(pub UnboundedSender<Arc<dyn kosong::ChatProvider>>);

impl std::fmt::Debug for ProviderRefreshSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRefreshSender").finish()
    }
}

/// Snapshot of MCP loading state.
#[derive(Debug, Clone, Default)]
pub struct MCPStatusSnapshot {
    pub loading: bool,
    pub connected: usize,
    pub total: usize,
    pub tools: usize,
}

/// An event emitted by a Brain turn.
#[derive(Debug, Clone)]
pub enum BrainEvent {
    /// Start of a turn.
    TurnBegin,

    /// Start of a single reasoning step within a turn.
    StepBegin { n: usize },

    /// End of a single reasoning step.
    StepEnd { n: usize },

    /// A step failed and will be retried.
    StepRetry {
        n: usize,
        next_attempt: usize,
        max_attempts: usize,
        wait_s: f64,
        error_type: BrainErrorCategory,
        status_code: Option<u16>,
    },

    /// A step was interrupted after exhausting retries/recovery.
    StepInterrupted,

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

    /// Token usage reported by the provider for a step.
    Usage { usage: kosong::TokenUsage },

    /// Status update with context/token accounting.
    StatusUpdate {
        context_usage: Option<f64>,
        context_tokens: Option<usize>,
        max_context_tokens: Option<usize>,
        token_usage: Option<kosong::TokenUsage>,
        plan_mode: Option<bool>,
    },

    /// MCP tool loading started.
    McpLoadingBegin,

    /// MCP tool loading finished.
    McpLoadingEnd { snapshot: MCPStatusSnapshot },

    /// A checkpoint was created.
    CheckpointCreated { id: CheckpointId },

    /// The history was reverted to a checkpoint.
    CheckpointReverted { id: CheckpointId },

    /// The provider is being refreshed automatically.
    ProviderRefreshing { reason: String },

    /// The frontend must provide a new provider interactively.
    ProviderRefreshRequested {
        reason: String,
        sender: ProviderRefreshSender,
    },

    /// The provider was refreshed successfully.
    ProviderRefreshed,

    /// End of a turn.
    TurnEnd,

    /// An error occurred during the turn.
    Error(String),
}

/// Allows applications to filter or transform Brain events before they are
/// emitted to the stream.
pub trait EventPolicy: Send + Sync {
    fn map(&self, event: BrainEvent) -> Option<BrainEvent>;
}

/// Default event policy that emits every event unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpEventPolicy;

impl EventPolicy for NoOpEventPolicy {
    fn map(&self, event: BrainEvent) -> Option<BrainEvent> {
        Some(event)
    }
}
