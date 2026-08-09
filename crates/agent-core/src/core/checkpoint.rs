use async_trait::async_trait;
use llm_provider::message::Message;

use crate::core::errors::BrainError;
use crate::core::events::CheckpointId;
use crate::core::step::StepContext;

/// Stores and restores conversation checkpoints.
#[async_trait]
pub trait CheckpointPolicy: Send + Sync {
    /// Create a checkpoint for the current history and return its id.
    async fn checkpoint(
        &self,
        ctx: &StepContext,
        history: &[Message],
    ) -> Result<CheckpointId, BrainError>;

    /// Revert to a checkpoint and return the history at that point.
    async fn revert_to(&self, id: CheckpointId) -> Result<Vec<Message>, BrainError>;

    /// Return the current checkpoint id, if any.
    async fn current(&self) -> Option<CheckpointId>;
}

/// No-op checkpoint policy that always returns checkpoint id 0 and never
/// actually reverts.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpCheckpointPolicy;

#[async_trait]
impl CheckpointPolicy for NoOpCheckpointPolicy {
    async fn checkpoint(
        &self,
        _ctx: &StepContext,
        _history: &[Message],
    ) -> Result<CheckpointId, BrainError> {
        Ok(0)
    }

    async fn revert_to(&self, _id: CheckpointId) -> Result<Vec<Message>, BrainError> {
        Ok(Vec::new())
    }

    async fn current(&self) -> Option<CheckpointId> {
        None
    }
}
