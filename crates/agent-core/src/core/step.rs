use async_trait::async_trait;
use llm_provider::message::Message;
use llm_provider::tooling::ToolResult as KosongToolResult;

use crate::core::errors::BrainError;

/// Context passed to step lifecycle hooks.
#[derive(Debug, Clone)]
pub struct StepContext {
    pub step_no: usize,
    pub turn_id: Option<String>,
}

/// Outcome of a single LLM step.
#[derive(Debug, Clone)]
pub enum StepOutcome {
    /// The step executed tool calls; the caller should decide whether to continue.
    Continue,
    /// The step produced a final assistant message.
    Final { text: String },
}

/// Direction returned by [`StepPolicy::after_step`].
#[derive(Debug, Clone)]
pub enum StepControl {
    /// Continue to the next step.
    Continue,
    /// Stop the turn and return the given final text.
    Stop { final_text: String },
    /// Revert to a checkpoint and inject messages before continuing.
    RewindToCheckpoint {
        checkpoint_id: crate::core::events::CheckpointId,
        inject_messages: Vec<Message>,
    },
}

/// Hooks into the step lifecycle.
///
/// Applications use this to inject messages before a step, decide whether to
/// continue after a step, and trigger checkpoint rewinds (e.g. D-Mail).
#[async_trait]
pub trait StepPolicy: Send + Sync {
    /// Called before each LLM step. Can mutate `history` to add reminders,
    /// notifications, steers, etc.
    async fn before_step(
        &self,
        _ctx: &StepContext,
        _history: &mut Vec<Message>,
    ) -> Result<(), BrainError> {
        Ok(())
    }

    /// Called after a step succeeds. Return [`StepControl::Continue`],
    /// [`StepControl::Stop`], or [`StepControl::RewindToCheckpoint`].
    async fn after_step(
        &self,
        _ctx: &StepContext,
        _outcome: &StepOutcome,
        _tool_results: &[KosongToolResult],
    ) -> Result<StepControl, BrainError> {
        match _outcome {
            StepOutcome::Continue => Ok(StepControl::Continue),
            StepOutcome::Final { text } => Ok(StepControl::Stop {
                final_text: text.clone(),
            }),
        }
    }
}

/// Default no-op step policy.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpStepPolicy;

#[async_trait]
impl StepPolicy for NoOpStepPolicy {}
