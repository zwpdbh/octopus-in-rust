use async_trait::async_trait;
use kosong::Tool;
use kosong::message::Message;

use crate::core::errors::BrainError;

/// Builds the effective system prompt for a step.
#[async_trait]
pub trait SystemPromptPolicy: Send + Sync {
    async fn build_prompt(
        &self,
        base: &str,
        tools: &[Tool],
        history: &[Message],
    ) -> Result<String, BrainError>;
}

/// Default policy that returns `base` unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultSystemPromptPolicy;

#[async_trait]
impl SystemPromptPolicy for DefaultSystemPromptPolicy {
    async fn build_prompt(
        &self,
        base: &str,
        _tools: &[Tool],
        _history: &[Message],
    ) -> Result<String, BrainError> {
        Ok(base.to_string())
    }
}
