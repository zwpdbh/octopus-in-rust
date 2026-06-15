use serde_json::Value;

/// Action a hook policy can request in response to a lifecycle event.
#[derive(Debug, Clone)]
pub enum HookAction {
    /// Allow the operation to proceed.
    Allow,
    /// Block the operation, optionally with a reason shown to the LLM/user.
    Block { reason: String },
}

/// Observes and optionally blocks agent lifecycle events.
#[async_trait::async_trait]
pub trait HookPolicy: Send + Sync {
    /// Called when the user submits a prompt, before the turn begins.
    async fn on_user_prompt_submit(&self, _prompt: &str) -> HookAction {
        HookAction::Allow
    }

    /// Called before a tool is executed.
    async fn on_pre_tool_use(
        &self,
        _tool_name: &str,
        _tool_input: &Value,
        _tool_call_id: &str,
    ) -> HookAction {
        HookAction::Allow
    }

    /// Called after a tool succeeds.
    async fn on_post_tool_use(
        &self,
        _tool_name: &str,
        _tool_input: &Value,
        _tool_output: &str,
        _tool_call_id: &str,
    ) {
    }

    /// Called after a tool fails.
    async fn on_post_tool_use_failure(
        &self,
        _tool_name: &str,
        _tool_input: &Value,
        _error: &str,
        _tool_call_id: &str,
    ) {
    }

    /// Called when a turn completes normally.
    async fn on_turn_end(&self, _final_text: &str) {}

    /// Called when a turn fails.
    async fn on_turn_failure(&self, _error: &str) {}
}

/// No-op hook policy that allows every event.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpHookPolicy;

#[async_trait::async_trait]
impl HookPolicy for NoOpHookPolicy {}
