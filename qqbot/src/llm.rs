use crate::core_config::CoreConfigFile;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelEntry {
    id: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
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
    #[allow(dead_code)]
    typ: String,
}

pub async fn ask(
    data_dir: &Path,
    prompt: &str,
    model_override: Option<&str>,
    base_url_override: Option<&str>,
) -> Result<()> {
    let config = CoreConfigFile::from_file(data_dir.join("config.toml"))?;
    let llm = &config.llm;
    let api_key = llm.api_key.trim();

    println!("=== qqbot llm ask ===\n");

    if llm.api_url.is_empty() || api_key.is_empty() || llm.model.is_empty() {
        println!("[fail] LLM is not fully configured in config.toml");
        return Ok(());
    }

    let chat_url = build_chat_url(base_url_override.unwrap_or(&llm.api_url));
    let model = model_override.unwrap_or(&llm.model);
    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![
            Message {
                role: "system".to_string(),
                content: llm.system_prompt.clone(),
            },
            Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            },
        ],
        max_tokens: Some(512),
    };

    println!("Endpoint: {chat_url}");
    println!("Model:    {model}");
    println!("Prompt:   {prompt}\n");

    let client = reqwest::Client::new();
    let resp = client
        .post(&chat_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .with_context(|| format!("failed to POST {chat_url}"))?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read response body")?;

    if !status.is_success() {
        println!("[fail] LLM API returned HTTP {status}");
        if let Ok(err) = serde_json::from_str::<LlmErrorResponse>(&body) {
            println!("       Error: {} ({})", err.error.message, err.error.typ);
        } else {
            println!("       Response: {body}");
        }
        return Ok(());
    }

    let chat: ChatResponse = match serde_json::from_str(&body) {
        Ok(c) => c,
        Err(e) => {
            println!("[fail] Failed to parse LLM response: {e}");
            println!("       Raw response: {body}");
            return Ok(());
        }
    };

    match chat.choices.into_iter().next() {
        Some(choice) => {
            println!("Reply:");
            println!("{}", choice.message.content);
        }
        None => println!("[fail] LLM returned no choices"),
    }

    Ok(())
}

pub async fn test(data_dir: &Path, base_url_override: Option<&str>) -> Result<()> {
    let config = CoreConfigFile::from_file(data_dir.join("config.toml"))?;
    let llm = &config.llm;
    let api_key = llm.api_key.trim();

    println!("=== qqbot llm test ===\n");

    if llm.api_url.is_empty() {
        println!("[fail] llm.api_url is not configured");
        return Ok(());
    }
    if api_key.is_empty() {
        println!("[fail] llm.api_key is not configured");
        return Ok(());
    }
    if llm.model.is_empty() {
        println!("[fail] llm.model is not configured");
        return Ok(());
    }

    let api_url = base_url_override.unwrap_or(&llm.api_url);
    let models_url = match build_models_url(api_url) {
        Ok(url) => url,
        Err(e) => {
            println!("[fail] Cannot derive models endpoint from api_url: {e}");
            return Ok(());
        }
    };

    let key_hint = if api_key.len() > 4 {
        format!("{}...", &api_key[..4])
    } else {
        "...".to_string()
    };
    println!("[ok]   endpoint: {}", api_url);
    println!("[ok]   models:   {}", models_url);
    println!("[ok]   api_key:  {key_hint}");
    println!("[ok]   model:    {}", llm.model);

    let client = reqwest::Client::new();
    let resp = client
        .get(&models_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .with_context(|| format!("failed to request {models_url}"))?;

    let status = resp.status();
    let body = resp.text().await.context("failed to read response body")?;

    if !status.is_success() {
        println!();
        println!("[fail] LLM API returned HTTP {status}");
        println!("       Response: {body}");
        println!();
        println!("       This usually means the API key is invalid or expired.");
        println!(
            "       Update llm.api_key in {} and run `qqbot restart`.",
            data_dir.join("config.toml").display()
        );
        return Ok(());
    }

    let models: ModelsResponse = match serde_json::from_str(&body) {
        Ok(m) => m,
        Err(e) => {
            println!();
            println!(
                "[warn] Authentication succeeded, but the models response could not be parsed: {e}"
            );
            println!("       Response: {body}");
            return Ok(());
        }
    };

    let ids: Vec<String> = models.data.into_iter().map(|m| m.id).collect();
    println!();
    println!("[ok]   API key is valid; {} model(s) available", ids.len());

    if ids.contains(&llm.model) {
        println!("[ok]   Configured model '{}' is available", llm.model);
    } else {
        println!(
            "[warn] Configured model '{}' was not found in the available models list",
            llm.model
        );
        println!("       Available models:");
        for id in ids.iter().take(20) {
            println!("         - {id}");
        }
        if ids.len() > 20 {
            println!("         ... and {} more", ids.len() - 20);
        }
    }

    Ok(())
}

fn build_models_url(api_url: &str) -> Result<String> {
    let base = if api_url.ends_with("/chat/completions") {
        &api_url[..api_url.len() - "/chat/completions".len()]
    } else {
        api_url
    };
    let base = base.trim_end_matches('/');
    Ok(format!("{base}/models"))
}

fn build_chat_url(api_url: &str) -> String {
    if api_url.ends_with("/chat/completions") {
        api_url.to_string()
    } else {
        let base = api_url.trim_end_matches('/');
        format!("{base}/chat/completions")
    }
}
