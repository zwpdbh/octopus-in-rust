use std::sync::Arc;

use llm_provider::tooling::DisplayBlock;
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

/// Runtime interface for approval decisions.
///
/// This is the boundary Brain uses. The default implementation wraps any
/// [`ApprovalPolicy`] and emits `ApprovalRequested`/`ApprovalResolved` events
/// around the policy call. Interactive frontends can implement this trait
/// directly to resolve approvals asynchronously via their own UI channels.
#[async_trait::async_trait]
pub trait ApprovalRuntime: Send + Sync {
    async fn request(
        &self,
        request: ApprovalRequest,
        events: tokio::sync::mpsc::UnboundedSender<crate::core::events::BrainEvent>,
    ) -> ApprovalResponse;
}

/// Wrapper that turns a simple [`ApprovalPolicy`] into an [`ApprovalRuntime`].
pub struct DefaultApprovalRuntime {
    policy: Arc<dyn ApprovalPolicy>,
}

impl DefaultApprovalRuntime {
    pub fn new(policy: Arc<dyn ApprovalPolicy>) -> Self {
        Self { policy }
    }
}

#[async_trait::async_trait]
impl ApprovalRuntime for DefaultApprovalRuntime {
    async fn request(
        &self,
        request: ApprovalRequest,
        events: tokio::sync::mpsc::UnboundedSender<crate::core::events::BrainEvent>,
    ) -> ApprovalResponse {
        let tool_call_id = request.tool_call_id.clone();
        let tool_name = request.tool_name.clone();
        let arguments = request.tool_input.clone();

        let _ = events.send(crate::core::events::BrainEvent::ApprovalRequested {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            arguments,
        });

        let response = self.policy.request(request).await;
        let approved = response.is_approved();
        let reason = match &response {
            ApprovalResponse::Approved => None,
            ApprovalResponse::Rejected { feedback } => Some(feedback.clone()),
        };

        let _ = events.send(crate::core::events::BrainEvent::ApprovalResolved {
            tool_call_id,
            approved,
            reason,
        });

        response
    }
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
