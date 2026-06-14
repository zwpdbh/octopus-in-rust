use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Clone, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmErrorResponse {
    error: LlmErrorDetail,
}

#[derive(Debug, Clone, Deserialize)]
struct LlmErrorDetail {
    message: String,
    #[serde(rename = "type")]
    typ: String,
}

pub struct LlmClient {
    client: reqwest::Client,
    api_url: String,
    api_key: String,
    model: String,
    system_prompt: String,
}

impl LlmClient {
    pub fn new(api_url: String, api_key: String, model: String, system_prompt: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_url,
            api_key,
            model,
            system_prompt,
        }
    }

    pub async fn chat(&self, user_prompt: &str, request_id: &str) -> Result<String> {
        let api_key = self.api_key.trim();
        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: self.system_prompt.clone(),
                },
                Message {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
        };

        debug!(request_id, model = %self.model, "sending LLM request");

        let resp = self
            .client
            .post(&self.api_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("LLM request failed")?;

        let status = resp.status();
        let raw = resp
            .text()
            .await
            .context("failed to read LLM response body")?;

        debug!(request_id, status = %status, body = %raw, "received LLM response");

        if status.is_client_error() || status.is_server_error() {
            if let Ok(err) = serde_json::from_str::<LlmErrorResponse>(&raw) {
                error!(
                    request_id,
                    error_message = %err.error.message,
                    error_type = %err.error.typ,
                    status = %status,
                    "LLM API returned an error"
                );
                anyhow::bail!("LLM API error: {} ({})", err.error.message, err.error.typ);
            }
            anyhow::bail!("LLM API returned HTTP {}", status);
        }

        let response: ChatResponse = serde_json::from_str(&raw).map_err(|e| {
            error!(request_id, error = %e, body = %raw, "failed to parse LLM response");
            anyhow::anyhow!("failed to parse LLM response")
        })?;

        response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .context("LLM returned no choices")
    }
}
