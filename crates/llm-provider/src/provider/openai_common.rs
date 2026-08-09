use crate::chat_provider::{ChatProviderError, convert_httpx_error};
use crate::provider::openai_types::ChatCompletionTool;
use crate::tooling::Tool;

/// Convert an llm-provider Tool to OpenAI tool format.
pub fn tool_to_openai(tool: &Tool) -> ChatCompletionTool {
    ChatCompletionTool {
        tool_type: "function".to_string(),
        function: crate::provider::openai_types::FunctionDefinition {
            name: tool.name.clone(),
            description: Some(tool.description.clone()),
            parameters: tool.parameters.clone(),
        },
    }
}

/// Convert a reqwest error into an llm-provider error.
pub fn convert_reqwest_error(err: reqwest::Error) -> ChatProviderError {
    if err.is_timeout() {
        return ChatProviderError::timeout(err.to_string());
    }
    if err.is_connect() || err.is_request() {
        return ChatProviderError::connection(err.to_string());
    }
    convert_httpx_error(&err)
}

/// Convert an HTTP status error into an llm-provider error.
pub fn convert_status_error(
    status: reqwest::StatusCode,
    body: String,
    request_id: Option<String>,
) -> ChatProviderError {
    ChatProviderError::status(status.as_u16(), body, request_id)
}

/// Map llm-provider ThinkingEffort to OpenAI reasoning_effort string.
pub fn thinking_effort_to_reasoning_effort(effort: &str) -> Option<String> {
    match effort {
        "off" => None,
        "low" => Some("low".to_string()),
        "medium" => Some("medium".to_string()),
        "high" => Some("high".to_string()),
        "xhigh" => Some("xhigh".to_string()),
        "max" => Some("xhigh".to_string()),
        _ => None,
    }
}

/// Map OpenAI reasoning_effort string to llm-provider ThinkingEffort.
pub fn reasoning_effort_to_thinking_effort(effort: Option<&str>) -> String {
    match effort {
        Some("low") | Some("minimal") => "low".to_string(),
        Some("medium") => "medium".to_string(),
        Some("high") => "high".to_string(),
        Some("xhigh") => "xhigh".to_string(),
        Some("none") | None => "off".to_string(),
        _ => "off".to_string(),
    }
}
