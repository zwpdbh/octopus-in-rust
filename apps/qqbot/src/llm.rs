use crate::core_config::{AuthConfig, CoreConfigFile, LlmConfig, LlmProviderConfig};
use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
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
struct ChatCompletionChunk {
    choices: Vec<ChunkChoice>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
}

#[derive(Debug, Clone, Deserialize)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    role: Option<String>,
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

    println!("=== qqbot llm ask ===\n");

    let (api_key, api_url, headers) = resolve_provider(llm).await?;

    if api_url.is_empty() || llm.model.is_empty() {
        println!("[fail] LLM is not fully configured in config.toml");
        return Ok(());
    }

    let chat_url = build_chat_url(base_url_override.unwrap_or(&api_url));
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
        stream: None,
    };

    println!("Endpoint: {chat_url}");
    println!("Model:    {model}");
    println!("Prompt:   {prompt}\n");

    let client = reqwest::Client::new();
    let mut req = client
        .post(&chat_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json");
    for (name, value) in &headers {
        req = req.header(name, value);
    }
    let resp = req
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

pub async fn stream(
    data_dir: &Path,
    prompt: &str,
    model_override: Option<&str>,
    base_url_override: Option<&str>,
) -> Result<()> {
    let config = CoreConfigFile::from_file(data_dir.join("config.toml"))?;
    let llm = &config.llm;

    println!("=== qqbot llm stream ===\n");

    let (api_key, api_url, headers) = resolve_provider(llm).await?;

    if api_url.is_empty() || llm.model.is_empty() {
        println!("[fail] LLM is not fully configured");
        return Ok(());
    }

    let chat_url = build_chat_url(base_url_override.unwrap_or(&api_url));
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
        stream: Some(true),
    };

    println!("[config] endpoint: {chat_url}");
    println!("[config] model:    {model}");
    println!("[config] prompt:   {prompt}\n");

    println!("[send] POST {chat_url}");

    let client = reqwest::Client::new();
    let mut req = client
        .post(&chat_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json");
    for (name, value) in &headers {
        req = req.header(name, value);
    }
    let resp = req
        .json(&request)
        .send()
        .await
        .with_context(|| format!("failed to POST {chat_url}"))?;

    let status = resp.status();
    println!("[headers] HTTP {status}");

    if !status.is_success() {
        let body = resp.text().await.context("failed to read error body")?;
        println!("[fail] LLM API returned HTTP {status}");
        if let Ok(err) = serde_json::from_str::<LlmErrorResponse>(&body) {
            println!("       Error: {} ({})", err.error.message, err.error.typ);
        } else {
            println!("       Response: {body}");
        }
        return Ok(());
    }

    println!("[streaming] waiting for first chunk...\n");
    println!("--- reply ---");

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut started = false;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read stream chunk")?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some((event, rest)) = buffer.split_once("\n\n") {
            let event = event.trim_end_matches('\r').to_string();
            buffer = rest.to_string();
            if let Some(data) = event.strip_prefix("data: ") {
                if data == "[DONE]" {
                    println!("\n--- end ---");
                    println!("[done]");
                    return Ok(());
                }
                match serde_json::from_str::<ChatCompletionChunk>(data) {
                    Ok(chunk) => {
                        for choice in chunk.choices {
                            if let Some(content) = choice.delta.content {
                                if !content.is_empty() {
                                    if !started {
                                        started = true;
                                        println!("[first chunk received]");
                                    }
                                    print!("{content}");
                                    stdout().flush()?;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("\n[warn] failed to parse chunk: {e}");
                        eprintln!("       raw: {data}");
                    }
                }
            }
        }
    }

    println!("\n--- end ---");
    println!("[done] stream closed");
    Ok(())
}

pub async fn test(data_dir: &Path, base_url_override: Option<&str>) -> Result<()> {
    let config = CoreConfigFile::from_file(data_dir.join("config.toml"))?;
    let llm = &config.llm;

    println!("=== qqbot llm test ===\n");

    let (api_key, api_url, headers) = resolve_provider(llm).await?;

    if api_url.is_empty() {
        println!("[fail] llm.api_url is not configured");
        return Ok(());
    }

    if llm.model.is_empty() {
        println!("[fail] llm.model is not configured");
        return Ok(());
    }

    let api_url = base_url_override.unwrap_or(&api_url);
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
    let mut req = client
        .get(&models_url)
        .header("Authorization", format!("Bearer {}", api_key));
    for (name, value) in &headers {
        req = req.header(name, value);
    }
    let resp = req
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
            "       Update llm.auth in {} and run `qqbot restart`.",
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

async fn resolve_provider(llm: &LlmConfig) -> Result<(String, String, HashMap<String, String>)> {
    match &llm.provider {
        LlmProviderConfig::OpenAiCompatible { api_url, auth } => {
            let token = resolve_auth(auth).await?;
            Ok((token, api_url.clone(), HashMap::new()))
        }
        LlmProviderConfig::KimiCode {
            api_url,
            token_file,
            identity,
        } => {
            let token = resolve_oauth_token(token_file).await?;
            let provider_identity = agent_core::ProviderIdentity {
                platform: "kimi_code_cli".to_string(),
                version: identity.version.clone(),
                user_agent_product: identity.user_agent_product.clone(),
                home_dir: expand_path(&identity.home_dir),
            };
            let headers =
                agent_core::core::provider::build_identity_headers(&provider_identity).await?;
            Ok((token, api_url.clone(), headers))
        }
    }
}

async fn resolve_auth(auth: &AuthConfig) -> Result<String> {
    match auth {
        AuthConfig::ApiKey { api_key } => Ok(api_key.clone()),
        AuthConfig::OAuth { token_file } => resolve_oauth_token(token_file).await,
    }
}

async fn resolve_oauth_token(token_file: &str) -> Result<String> {
    let manager = agent_core::OAuthManager::new(agent_core::OAuthConfig::kimi_code(), token_file);
    manager.access_token().await
}

fn build_models_url(api_url: &str) -> Result<String> {
    let base = api_url.strip_suffix("/chat/completions").unwrap_or(api_url);
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

fn expand_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}
