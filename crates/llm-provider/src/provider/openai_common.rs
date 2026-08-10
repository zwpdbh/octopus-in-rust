use crate::chat_provider::{ChatProviderError, convert_httpx_error};
use crate::provider::openai_types::{ChatCompletionTool, ModelsResponse};
use crate::tooling::Tool;
use std::collections::HashMap;

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

/// Derive the API root URL from a provider's base URL.
///
/// Some configs set the base URL to the chat-completions endpoint directly
/// (e.g. `https://api.openai.com/v1/chat/completions`). This strips that
/// suffix and any trailing slash so we can append `/models`.
pub fn openai_api_root(base_url: &str) -> String {
    base_url
        .strip_suffix("/chat/completions")
        .unwrap_or(base_url)
        .trim_end_matches('/')
        .to_string()
}

/// List models from an OpenAI-compatible `/models` endpoint.
pub async fn list_openai_models(
    http_client: &reqwest::Client,
    base_url: &str,
    api_key: Option<&str>,
    extra_headers: &HashMap<String, String>,
) -> Result<Vec<String>, ChatProviderError> {
    let url = format!("{}/models", openai_api_root(base_url));

    let mut req = http_client.get(&url);
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    for (name, value) in extra_headers {
        req = req.header(name, value);
    }

    let response = req.send().await.map_err(convert_reqwest_error)?;

    let status = response.status();
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(convert_status_error(status, body, request_id));
    }

    let body = response.text().await.map_err(convert_reqwest_error)?;
    let models: ModelsResponse = serde_json::from_str(&body)
        .map_err(|e| ChatProviderError::new(format!("Failed to parse models response: {e}")))?;

    Ok(models.data.into_iter().map(|m| m.id).collect())
}
