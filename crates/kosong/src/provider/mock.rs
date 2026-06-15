use crate::chat_provider::{ChatProvider, Part, StreamedMessage, ThinkingEffort};
use crate::message::Message;
use crate::tooling::Tool;
use async_trait::async_trait;
use std::sync::Arc;

/// A mock chat provider that always returns predefined message parts.
///
/// Useful for unit testing kosong consumers without making real API calls.
#[derive(Debug, Clone)]
pub struct MockChatProvider {
    message_parts: Vec<Part>,
}

impl MockChatProvider {
    pub fn new(message_parts: Vec<Part>) -> Self {
        Self { message_parts }
    }
}

#[async_trait]
impl ChatProvider for MockChatProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn model_name(&self) -> &str {
        "mock"
    }

    fn thinking_effort(&self) -> Option<&ThinkingEffort> {
        None
    }

    async fn generate(
        &self,
        _system_prompt: &str,
        _tools: &[Tool],
        _history: &[Message],
    ) -> Result<StreamedMessage, crate::chat_provider::ChatProviderError> {
        Ok(StreamedMessage {
            id: Some("mock".to_string()),
            usage: None,
            stream: Box::pin(futures::stream::iter(self.message_parts.clone())),
        })
    }

    fn with_thinking(&self, _effort: ThinkingEffort) -> Arc<dyn ChatProvider> {
        Arc::new(self.clone())
    }
}
