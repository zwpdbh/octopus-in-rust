use crate::chat_provider::{
    ChatProvider, ChatProviderError, Part, StreamedMessage, ThinkingEffort,
};
use crate::message::{Message, Role, TokenUsage};
use crate::tooling::Tool;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

mod dsl;

// ============================================================================
// EchoChatProvider
// ============================================================================

/// A test-only chat provider that streams parts described by a tiny DSL.
///
/// The DSL lives in the content of the last message in `history` and is made of lines in the
/// form `kind: payload`. See [`dsl::parse_echo_script`] for the full DSL specification.
#[derive(Debug, Clone)]
pub struct EchoChatProvider;

impl EchoChatProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EchoChatProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for EchoChatProvider {
    fn name(&self) -> &str {
        "echo"
    }

    fn model_name(&self) -> &str {
        "echo"
    }

    fn thinking_effort(&self) -> Option<&ThinkingEffort> {
        None
    }

    async fn generate(
        &self,
        _system_prompt: &str,
        _tools: &[Tool],
        history: &[Message],
    ) -> Result<StreamedMessage, ChatProviderError> {
        if history.is_empty() {
            return Err(ChatProviderError::new(
                "EchoChatProvider requires at least one message in history.",
            ));
        }
        let last = history.last().unwrap();
        if last.role != Role::User {
            return Err(ChatProviderError::new(
                "EchoChatProvider expects the last history message to be user.",
            ));
        }

        let script_text = last.extract_text("\n");
        let (parts, message_id, usage) = dsl::parse_echo_script(&script_text)?;
        if parts.is_empty() {
            return Err(ChatProviderError::new(
                "EchoChatProvider DSL produced no streamable parts.",
            ));
        }

        Ok(build_streamed_message(parts, message_id, usage))
    }

    async fn list_models(&self) -> Result<Vec<String>, ChatProviderError> {
        Ok(vec!["echo".to_string()])
    }

    fn with_thinking(&self, _effort: ThinkingEffort) -> Arc<dyn ChatProvider> {
        Arc::new(self.clone())
    }
}

// ============================================================================
// ScriptedEchoChatProvider
// ============================================================================

/// A test-only chat provider that consumes a queue of echo DSL scripts per call.
#[derive(Debug, Clone)]
pub struct ScriptedEchoChatProvider {
    scripts: Arc<Mutex<VecDeque<String>>>,
    turn: Arc<Mutex<usize>>,
    trace: bool,
}

impl ScriptedEchoChatProvider {
    pub fn new(scripts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into_iter().map(|s| s.into()).collect())),
            turn: Arc::new(Mutex::new(0)),
            trace: false,
        }
    }

    pub fn with_trace(mut self, trace: bool) -> Self {
        self.trace = trace;
        self
    }
}

#[async_trait]
impl ChatProvider for ScriptedEchoChatProvider {
    fn name(&self) -> &str {
        "scripted_echo"
    }

    fn model_name(&self) -> &str {
        "scripted_echo"
    }

    fn thinking_effort(&self) -> Option<&ThinkingEffort> {
        None
    }

    async fn generate(
        &self,
        _system_prompt: &str,
        _tools: &[Tool],
        _history: &[Message],
    ) -> Result<StreamedMessage, ChatProviderError> {
        let mut scripts = self.scripts.lock().unwrap();
        let mut turn = self.turn.lock().unwrap();

        let script_text = scripts.pop_front().ok_or_else(|| {
            ChatProviderError::new(format!(
                "ScriptedEchoChatProvider exhausted at turn {}.",
                *turn + 1
            ))
        })?;

        if self.trace {
            let script_json = serde_json::to_string(&script_text).unwrap_or_default();
            eprintln!("SCRIPTED_ECHO TURN {}: {}", *turn + 1, script_json);
        }

        *turn += 1;
        drop(scripts);
        drop(turn);

        let (parts, message_id, usage) = dsl::parse_echo_script(&script_text)?;
        if parts.is_empty() {
            return Err(ChatProviderError::new(
                "ScriptedEchoChatProvider DSL produced no streamable parts.",
            ));
        }

        Ok(build_streamed_message(parts, message_id, usage))
    }

    async fn list_models(&self) -> Result<Vec<String>, ChatProviderError> {
        Ok(vec!["scripted_echo".to_string()])
    }

    fn with_thinking(&self, _effort: ThinkingEffort) -> Arc<dyn ChatProvider> {
        let mut cloned = self.clone();
        cloned.scripts = Arc::new(Mutex::new(self.scripts.lock().unwrap().clone()));
        cloned.turn = Arc::new(Mutex::new(*self.turn.lock().unwrap()));
        Arc::new(cloned)
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn build_streamed_message(
    parts: Vec<Part>,
    message_id: Option<String>,
    usage: Option<TokenUsage>,
) -> StreamedMessage {
    StreamedMessage {
        id: message_id,
        usage,
        stream: Box::pin(futures::stream::iter(parts)),
    }
}
