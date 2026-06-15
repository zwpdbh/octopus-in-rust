use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use futures::{Stream, StreamExt};
use kosong::message::{ContentPart, Message, Role};
use kosong::provider::openai_legacy::OpenAILegacy;
use kosong::tooling::{HandleResult, ToolReturnValue};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::core::approval::{ApprovalPolicy, ApprovalRequest, ApprovalResponse};
use crate::core::config::BrainConfig;
use crate::core::events::BrainEvent;
use crate::core::registry::ToolRegistry;
use crate::core::turn::{TurnInput, TurnResult};

/// Errors that can occur while running the Brain.
#[derive(Debug, thiserror::Error)]
pub enum BrainError {
    #[error("No LLM provider configured")]
    NoProvider,
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("Tool error: {0}")]
    Tool(String),
    #[error("Turn exceeded maximum steps ({0})")]
    MaxSteps(usize),
}

/// A reusable agent core.
#[derive(Clone)]
pub struct Brain {
    config: BrainConfig,
    provider: Arc<dyn kosong::ChatProvider>,
    registry: ToolRegistry,
}

impl Brain {
    /// Create a new Brain from configuration.
    pub fn new(config: BrainConfig) -> Result<Self> {
        let provider = build_provider(&config)?;
        let registry = ToolRegistry::new();

        // Load tools from configured external sources.
        for source in &config.tool_sources {
            for tool in source.load_tools() {
                let name = tool.name().to_string();
                if registry.find(&name).is_some() {
                    tracing::warn!(
                        "Tool '{}' from source '{}' conflicts with an existing tool, skipping",
                        name,
                        source.name()
                    );
                    continue;
                }
                registry.register(tool);
                tracing::info!("Registered tool '{}' from source '{}'", name, source.name());
            }
        }

        Ok(Self {
            config,
            provider,
            registry,
        })
    }

    /// Access the underlying tool registry.
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    /// Access the message store.
    pub fn message_store(&self) -> &Arc<std::sync::Mutex<dyn crate::session::store::MessageStore>> {
        &self.config.message_store
    }

    /// Run a single user turn and return a stream of events.
    ///
    /// The stream emits `BrainEvent`s as they happen: `TurnBegin`, text/thinking
    /// fragments, tool calls, approval requests/resolutions, tool results, and
    /// finally `TurnEnd` or `Error`.
    pub async fn run_turn(
        &mut self,
        input: TurnInput,
    ) -> Result<Pin<Box<dyn Stream<Item = BrainEvent> + Send>>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let config = self.config.clone();
        let provider = self.provider.clone();
        let registry = self.registry.clone();

        tokio::spawn(async move {
            run_turn_loop(config, provider, registry, input, tx).await;
        });

        Ok(Box::pin(
            tokio_stream::wrappers::UnboundedReceiverStream::new(rx),
        ))
    }

    /// Run a single user turn to completion (non-streaming).
    ///
    /// Convenience wrapper around [`Self::run_turn`] that collects all events
    /// and returns the final text.
    pub async fn run_turn_to_completion(&mut self, input: TurnInput) -> Result<TurnResult> {
        let mut stream = self.run_turn(input).await?;
        let mut events = Vec::new();
        let mut final_text = String::new();

        while let Some(event) = stream.next().await {
            match &event {
                BrainEvent::TextPart(text) => final_text.push_str(text),
                BrainEvent::ToolCall { .. } => final_text.clear(),
                _ => {}
            }
            events.push(event);
        }

        Ok(TurnResult { events, final_text })
    }

    /// Register an additional tool at runtime (e.g. a host-provided tool).
    pub fn register_tool(&mut self, tool: Box<dyn kosong::tooling::CallableTool>) {
        let name = tool.name().to_string();
        if self.registry.find(&name).is_some() {
            tracing::warn!("Tool '{}' already registered, skipping", name);
            return;
        }
        self.registry.register(tool);
        tracing::info!("Registered host tool: {}", name);
    }

    /// Replace the current system prompt.
    pub fn set_system_prompt(&mut self, prompt: String) {
        self.config.system_prompt = prompt;
    }
}

async fn run_turn_loop(
    config: BrainConfig,
    provider: Arc<dyn kosong::ChatProvider>,
    registry: ToolRegistry,
    input: TurnInput,
    tx: UnboundedSender<BrainEvent>,
) {
    let _ = tx.send(BrainEvent::TurnBegin);

    config
        .hook_policy
        .on_user_prompt_submit(&input.user_message)
        .await;

    {
        let mut store = config.message_store.lock().unwrap();
        store.push(Message {
            role: Role::User,
            name: None,
            content: vec![ContentPart::Text {
                text: input.user_message,
            }],
            tool_calls: None,
            tool_call_id: None,
            partial: None,
        });
    }

    let toolset = ApprovalToolset {
        inner: registry.clone(),
        policy: config.approval_policy.clone(),
        event_tx: tx.clone(),
    };

    let mut final_text = String::new();

    for _step_no in 0..config.max_steps_per_turn {
        // Build the effective history for this step.
        let base_history = config.message_store.lock().unwrap().history().to_vec();

        // Optional compaction.
        let base_history = if let Some(policy) = &config.compaction_policy {
            if let Some(compacted) = policy.maybe_compact(&base_history).await {
                config
                    .message_store
                    .lock()
                    .unwrap()
                    .set_history(compacted.clone());
                compacted
            } else {
                base_history
            }
        } else {
            base_history
        };

        // Optional dynamic injection (not persisted).
        let injected = if let Some(policy) = &config.injection_policy {
            policy.inject(&base_history).await
        } else {
            Vec::new()
        };
        let step_history: Vec<Message> = injected
            .iter()
            .chain(base_history.iter())
            .cloned()
            .collect();

        let step_result = match kosong::step(
            provider.as_ref(),
            &config.system_prompt,
            &toolset,
            &step_history,
            None,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                let msg = e.message;
                let _ = tx.send(BrainEvent::Error(msg.clone()));
                let _ = tx.send(BrainEvent::TurnEnd);
                config.hook_policy.on_turn_failure(&msg).await;
                return;
            }
        };

        // Extract text / thinking / tool-call events from the assistant message.
        let assistant_message = step_result.message.clone();
        let mut assistant_text = String::new();
        for part in &assistant_message.content {
            match part {
                ContentPart::Text { text } => {
                    let _ = tx.send(BrainEvent::TextPart(text.clone()));
                    assistant_text.push_str(text);
                }
                ContentPart::Think { think, .. } => {
                    let _ = tx.send(BrainEvent::ThinkingPart(think.clone()));
                }
                _ => {}
            }
        }

        // If the assistant requested tool calls, emit events and execute them.
        if let Some(ref tool_calls) = assistant_message.tool_calls {
            // Persist the assistant message (with tool_calls) before awaiting tools.
            config
                .message_store
                .lock()
                .unwrap()
                .push(assistant_message.clone());

            for tc in tool_calls {
                let args = tc.function.arguments.clone().unwrap_or_default();
                let args_value = serde_json::from_str(&args).unwrap_or(Value::Null);

                let _ = tx.send(BrainEvent::ToolCall {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    arguments: args_value.clone(),
                });

                config
                    .hook_policy
                    .on_pre_tool_use(&tc.function.name, &args_value, &tc.id)
                    .await;
            }

            let results = step_result.tool_results().await;

            for result in results {
                let output = result
                    .return_value
                    .output
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let message = result
                    .return_value
                    .message
                    .clone()
                    .unwrap_or_else(|| output.clone());

                let _ = tx.send(BrainEvent::ToolResult {
                    id: result.tool_call_id.clone(),
                    output: message.clone(),
                    is_error: result.return_value.is_error,
                });

                append_tool_result(
                    &config.message_store,
                    &result.tool_call_id,
                    result.return_value.clone(),
                );

                if result.return_value.is_error {
                    config
                        .hook_policy
                        .on_post_tool_use_failure("", &Value::Null, &message, &result.tool_call_id)
                        .await;
                } else {
                    config
                        .hook_policy
                        .on_post_tool_use("", &Value::Null, &message, &result.tool_call_id)
                        .await;
                }
            }

            final_text.clear();
            continue;
        }

        // No tool calls: this is the final answer for the turn.
        config.message_store.lock().unwrap().push(assistant_message);
        final_text = assistant_text;
        break;
    }

    if final_text.is_empty() {
        let error = format!(
            "Turn exceeded maximum steps ({})",
            config.max_steps_per_turn
        );
        let _ = tx.send(BrainEvent::Error(error.clone()));
        let _ = tx.send(BrainEvent::TurnEnd);
        config.hook_policy.on_turn_failure(&error).await;
        return;
    }

    let _ = tx.send(BrainEvent::TurnEnd);
    config.hook_policy.on_turn_end(&final_text).await;
}

fn append_tool_result(
    message_store: &Arc<std::sync::Mutex<dyn crate::session::store::MessageStore>>,
    tool_call_id: &str,
    return_value: ToolReturnValue,
) {
    let message = return_value
        .message
        .clone()
        .or_else(|| {
            return_value
                .output
                .as_ref()
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_default();

    message_store.lock().unwrap().push(Message {
        role: Role::Tool,
        name: None,
        content: vec![ContentPart::Text { text: message }],
        tool_calls: None,
        tool_call_id: Some(tool_call_id.to_string()),
        partial: None,
    });
}

/// Toolset wrapper that gates each tool call through an [`ApprovalPolicy`].
struct ApprovalToolset {
    inner: ToolRegistry,
    policy: Arc<dyn ApprovalPolicy>,
    event_tx: UnboundedSender<BrainEvent>,
}

impl kosong::Toolset for ApprovalToolset {
    fn tools(&self) -> Vec<kosong::Tool> {
        self.inner.tools()
    }

    fn handle(&self, tool_call: &kosong::ToolCall) -> HandleResult {
        let policy = self.policy.clone();
        let inner = self.inner.clone();
        let event_tx = self.event_tx.clone();
        let tc = tool_call.clone();

        let handle = tokio::spawn(async move {
            let args_value = tc
                .function
                .arguments
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(Value::Null);

            let _ = event_tx.send(BrainEvent::ApprovalRequested {
                tool_call_id: tc.id.clone(),
                tool_name: tc.function.name.clone(),
                arguments: args_value.clone(),
            });

            let request = ApprovalRequest {
                tool_name: tc.function.name.clone(),
                tool_input: args_value,
                tool_call_id: tc.id.clone(),
                display: Vec::new(),
            };

            let response = policy.request(request).await;
            let approved = response.is_approved();
            let reason = match response {
                ApprovalResponse::Approved => None,
                ApprovalResponse::Rejected { feedback } => Some(feedback),
            };

            let _ = event_tx.send(BrainEvent::ApprovalResolved {
                tool_call_id: tc.id.clone(),
                approved,
                reason: reason.clone(),
            });

            if approved {
                match inner.handle(&tc) {
                    HandleResult::Ready(result) => result,
                    HandleResult::Pending(handle) => match handle.await {
                        Ok(result) => result,
                        Err(e) => kosong::tooling::ToolResult {
                            tool_call_id: tc.id,
                            return_value: kosong::tooling::ToolReturnValue::error(format!(
                                "Tool execution failed: {}",
                                e
                            )),
                        },
                    },
                }
            } else {
                kosong::tooling::ToolResult {
                    tool_call_id: tc.id,
                    return_value: kosong::tooling::ToolReturnValue::error(
                        reason.unwrap_or_else(|| "Tool call rejected".to_string()),
                    ),
                }
            }
        });

        HandleResult::Pending(handle)
    }
}

fn build_provider(config: &BrainConfig) -> Result<Arc<dyn kosong::ChatProvider>> {
    if config.base_url.is_empty() || config.model.is_empty() {
        return Err(BrainError::NoProvider.into());
    }

    let provider = OpenAILegacy::new(&config.model)
        .with_base_url(&config.base_url)
        .with_stream(false);

    let provider = if config.api_key.is_empty() {
        provider
    } else {
        provider.with_api_key(&config.api_key)
    };

    Ok(Arc::new(provider))
}
