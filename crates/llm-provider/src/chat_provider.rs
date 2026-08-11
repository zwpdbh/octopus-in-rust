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

    /// List the models available from this provider.
    async fn list_models(&self) -> Result<Vec<String>, ChatProviderError>;

    fn with_thinking(&self, effort: ThinkingEffort) -> Arc<dyn ChatProvider>;
}

/// A chat provider that can recreate itself on retryable errors.
pub trait RetryableChatProvider: ChatProvider {
    fn on_retryable_error(&self, error: &ChatProviderError) -> bool;
}

/// The kind of error that occurred while talking to a chat provider.
#[derive(Debug, Clone)]
pub enum ChatProviderErrorKind {
    /// A network-level connection failure.
    Connection(String),
    /// The request timed out.
    Timeout(String),
    /// The provider returned a non-success HTTP status.
    Status {
        status_code: u16,
        message: String,
        request_id: Option<String>,
    },
    /// The provider returned an empty response body.
    EmptyResponse,
    /// Any other provider error.
    Other(String),
}

/// Base error type for chat providers.
#[derive(Debug, Error, Clone)]
#[error("Chat provider error: {message}")]
pub struct ChatProviderError {
    pub kind: ChatProviderErrorKind,
    pub message: String,
}

impl ChatProviderError {
    /// Create a generic provider error from a message.
    pub fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            kind: ChatProviderErrorKind::Other(message.clone()),
            message,
        }
    }

    /// Create a connection error.
    pub fn connection(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            kind: ChatProviderErrorKind::Connection(message.clone()),
            message: format!("API connection error: {message}"),
        }
    }

    /// Create a timeout error.
    pub fn timeout(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            kind: ChatProviderErrorKind::Timeout(message.clone()),
            message: format!("API timeout error: {message}"),
        }
    }

    /// Create an HTTP status error.
    pub fn status(
        status_code: u16,
        message: impl Into<String>,
        request_id: Option<String>,
    ) -> Self {
        let message = message.into();
        Self {
            kind: ChatProviderErrorKind::Status {
                status_code,
                message: message.clone(),
                request_id: request_id.clone(),
            },
            message: format!(
                "API status error {status_code}: {message} (request_id={request_id:?})"
            ),
        }
    }

    /// Create an empty-response error.
    pub fn empty_response() -> Self {
        Self {
            kind: ChatProviderErrorKind::EmptyResponse,
            message: "API returned an empty response".to_string(),
        }
    }
}

/// Convert an HTTP / reqwest error into an llm-provider error.
pub fn convert_httpx_error(err: &reqwest::Error) -> ChatProviderError {
    if err.is_timeout() {
        return ChatProviderError::timeout(err.to_string());
    }
    if err.is_connect() || err.is_request() {
        return ChatProviderError::connection(err.to_string());
    }
    ChatProviderError::new(err.to_string())
}
