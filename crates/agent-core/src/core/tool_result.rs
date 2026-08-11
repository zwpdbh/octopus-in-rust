use async_trait::async_trait;
use llm_provider::tooling::ToolReturnValue;

use crate::core::errors::BrainError;

/// Transforms a tool return value before it is persisted to history.
#[async_trait]
pub trait ToolResultTransformer: Send + Sync {
    async fn transform(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        return_value: ToolReturnValue,
    ) -> Result<ToolReturnValue, BrainError>;
}

/// Default transformer that leaves tool results unchanged.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpToolResultTransformer;

#[async_trait]
impl ToolResultTransformer for NoOpToolResultTransformer {
    async fn transform(
        &self,
        _tool_call_id: &str,
        _tool_name: &str,
        return_value: ToolReturnValue,
    ) -> Result<ToolReturnValue, BrainError> {
        Ok(return_value)
    }
}
