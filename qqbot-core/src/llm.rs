use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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

    pub async fn chat(&self, user_prompt: &str) -> Result<String> {
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

        let response: ChatResponse = self
            .client
            .post(&self.api_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("LLM request failed")?
            .json()
            .await
            .context("failed to parse LLM response")?;

        response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .context("LLM returned no choices")
    }
}
