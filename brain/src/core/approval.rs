use kosong::tooling::DisplayBlock;
use serde_json::Value;

/// A request for approval before executing a tool call.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    /// Name of the tool being called.
    pub tool_name: String,

    /// Arguments passed to the tool.
    pub tool_input: Value,

    /// Provider-side tool call ID.
    pub tool_call_id: String,

    /// Optional display blocks describing the call for a UI.
    pub display: Vec<DisplayBlock>,
}

/// Response from an approval policy.
#[derive(Debug, Clone)]
pub enum ApprovalResponse {
    /// The tool call may proceed.
    Approved,
    /// The tool call is rejected, optionally with feedback to the LLM.
    Rejected { feedback: String },
}

impl ApprovalResponse {
    /// Whether the response is `Approved`.
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved)
    }
}

/// Decides whether a tool call may execute.
#[async_trait::async_trait]
pub trait ApprovalPolicy: Send + Sync {
    /// Called before each tool call. The Brain awaits the response before
    /// dispatching the call to the tool registry.
    async fn request(&self, request: ApprovalRequest) -> ApprovalResponse;
}

/// Default approval policy that approves every request.
///
/// This is the safe default for daemon consumers such as `qqbot-core` that have
/// no interactive approval UI.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutoApprove;

#[async_trait::async_trait]
impl ApprovalPolicy for AutoApprove {
    async fn request(&self, _request: ApprovalRequest) -> ApprovalResponse {
        ApprovalResponse::Approved
    }
}
