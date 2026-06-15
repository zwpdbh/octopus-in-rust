use crate::message::{ContentPart, Message, TokenUsage, ToolCall, ToolCallPart};
use crate::tooling::Tool;
use async_trait::async_trait;
use futures::stream::BoxStream;
use std::sync::Arc;
use thiserror::Error;

pub type StreamedMessagePart = Part;

/// A streamed part from the provider.
#[derive(Debug, Clone)]
pub enum Part {
    Content(ContentPart),
    ToolCall(ToolCall),
    ToolCallPart(ToolCallPart),
}

/// Thinking effort level.
pub type ThinkingEffort = String;

/// A stream of message parts from a chat provider, along with metadata.
pub struct StreamedMessage {
    pub id: Option<String>,
    pub usage: Option<TokenUsage>,
    pub stream: BoxStream<'static, Part>,
}

/// A chat provider that can generate messages.
#[async_trait]
pub trait ChatProvider: Send + Sync {
    fn name(&self) -> &str;
    fn model_name(&self) -> &str;
    fn thinking_effort(&self) -> Option<&ThinkingEffort>;

    async fn generate(
        &self,
        system_prompt: &str,
        tools: &[Tool],
        history: &[Message],
    ) -> Result<StreamedMessage, ChatProviderError>;

    fn with_thinking(&self, effort: ThinkingEffort) -> Arc<dyn ChatProvider>;
}

/// A chat provider that can recreate itself on retryable errors.
pub trait RetryableChatProvider: ChatProvider {
    fn on_retryable_error(&self, error: &ChatProviderError) -> bool;
}

/// Base error type for chat providers.
#[derive(Debug, Error, Clone)]
#[error("Chat provider error: {message}")]
pub struct ChatProviderError {
    pub message: String,
}

impl ChatProviderError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, Error, Clone)]
#[error("API connection error: {0}")]
pub struct APIConnectionError(pub String);

#[derive(Debug, Error, Clone)]
#[error("API timeout error: {0}")]
pub struct APITimeoutError(pub String);

#[derive(Debug, Error, Clone)]
#[error("API status error {status_code}: {message} (request_id={request_id:?})")]
pub struct APIStatusError {
    pub status_code: u16,
    pub message: String,
    pub request_id: Option<String>,
}

#[derive(Debug, Error, Clone)]
#[error("API returned an empty response")]
pub struct APIEmptyResponseError;

/// Convert an HTTP / reqwest error into a kosong error.
pub fn convert_httpx_error(err: &reqwest::Error) -> ChatProviderError {
    if err.is_timeout() {
        return ChatProviderError::new(APITimeoutError(err.to_string()).to_string());
    }
    if err.is_connect() || err.is_request() {
        return ChatProviderError::new(APIConnectionError(err.to_string()).to_string());
    }
    ChatProviderError::new(err.to_string())
}
