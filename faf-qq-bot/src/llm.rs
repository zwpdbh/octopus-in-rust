use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A single chat message for the LLM API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Clone, Deserialize)]
struct Choice {
    message: ChatMessage,
}

/// Summarize a conversation using an OpenAI-compatible chat completion endpoint.
pub async fn summarize(
    http: &reqwest::Client,
    api_url: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    conversation: &str,
) -> Result<String> {
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Please summarize the following QQ group conversation:\n\n{conversation}"
            ),
        },
    ];

    let request_body = ChatCompletionRequest {
        model: model.to_string(),
        messages,
    };

    let response = http
        .post(api_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .context("failed to call LLM API")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("LLM API returned error {status}: {body}");
    }

    let completion: ChatCompletionResponse = response
        .json()
        .await
        .context("failed to parse LLM response")?;

    completion
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .context("LLM response contained no choices")
}
