use crate::chat_provider::{ChatProvider, ChatProviderError, StreamedMessagePart};
use crate::generate::generate;
use crate::message::{Message, TokenUsage, ToolCall};
use crate::tooling::{HandleResult, ToolResult, Toolset};
use std::collections::HashMap;

/// Result of a single agent step.
pub struct StepResult {
    pub id: Option<String>,
    pub message: Message,
    pub usage: Option<TokenUsage>,
    pub tool_calls: Vec<ToolCall>,
    tool_result_futures: HashMap<String, tokio::task::JoinHandle<ToolResult>>,
}

impl StepResult {
    pub async fn tool_results(self) -> Vec<ToolResult> {
        if self.tool_result_futures.is_empty() {
            return Vec::new();
        }

        let mut results = Vec::new();
        let mut futures = self.tool_result_futures;

        for tool_call in &self.tool_calls {
            if let Some(handle) = futures.remove(&tool_call.id) {
                match handle.await {
                    Ok(result) => results.push(result),
                    Err(e) => {
                        results.push(ToolResult {
                            tool_call_id: tool_call.id.clone(),
                            return_value: crate::tooling::ToolReturnValue::error(format!(
                                "Tool execution failed: {e}"
                            )),
                        });
                    }
                }
            }
        }

        // Cancel any remaining futures
        for (_, handle) in futures {
            handle.abort();
        }

        results
    }
}

/// Run one agent step: generate an LLM response and dispatch tool calls.
pub async fn step(
    chat_provider: &dyn ChatProvider,
    system_prompt: &str,
    toolset: &dyn Toolset,
    history: &[Message],
    on_message_part: Option<&mut (dyn FnMut(StreamedMessagePart) + Send)>,
) -> Result<StepResult, ChatProviderError> {
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut tool_result_futures: HashMap<String, tokio::task::JoinHandle<ToolResult>> =
        HashMap::new();

    let mut on_tool_call = |tool_call: ToolCall| {
        let id = tool_call.id.clone();
        let result = toolset.handle(&tool_call);
        let handle = match result {
            HandleResult::Ready(result) => tokio::spawn(async move { result }),
            HandleResult::Pending(handle) => handle,
        };
        tool_calls.push(tool_call);
        tool_result_futures.insert(id, handle);
    };

    let result = generate(
        chat_provider,
        system_prompt,
        &toolset.tools(),
        history,
        on_message_part,
        Some(&mut on_tool_call),
    )
    .await;

    match result {
        Ok(gen_result) => Ok(StepResult {
            id: gen_result.id,
            message: gen_result.message,
            usage: gen_result.usage,
            tool_calls,
            tool_result_futures,
        }),
        Err(e) => {
            // Cancel all pending futures
            for (_, handle) in tool_result_futures {
                handle.abort();
            }
            Err(e)
        }
    }
}
