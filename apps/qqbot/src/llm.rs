use crate::core_config::{
    AuthConfig, CoreConfigFile, KimiCodeIdentity, LlmConfig, LlmProviderConfig,
};
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

const KIMI_CODE_PLATFORM: &str = "kimi_code_cli";

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
            let token = crate::oauth::resolve_token(token_file).await?;
            let headers = build_kimi_code_identity_headers(identity)?;
            Ok((token, api_url.clone(), headers))
        }
    }
}

async fn resolve_auth(auth: &AuthConfig) -> Result<String> {
    match auth {
        AuthConfig::ApiKey { api_key } => Ok(api_key.clone()),
        AuthConfig::OAuth { token_file } => crate::oauth::resolve_token(token_file).await,
    }
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

fn build_kimi_code_identity_headers(
    identity: &KimiCodeIdentity,
) -> Result<HashMap<String, String>> {
    let home_dir = expand_path(&identity.home_dir);

    let device_id = read_device_id(&home_dir);
    let hostname = ascii_header(std::env::var("HOSTNAME").as_deref().unwrap_or("qqbot"));
    let device_model = ascii_header(&format_device_model());
    let os_version = ascii_header(get_sys_release().as_str());

    let mut headers = HashMap::new();
    headers.insert(
        "User-Agent".to_string(),
        format!("{}/{}", identity.user_agent_product, identity.version),
    );
    headers.insert("X-Msh-Platform".to_string(), KIMI_CODE_PLATFORM.to_string());
    headers.insert("X-Msh-Version".to_string(), identity.version.clone());
    headers.insert("X-Msh-Device-Name".to_string(), hostname);
    headers.insert("X-Msh-Device-Model".to_string(), device_model);
    headers.insert("X-Msh-Os-Version".to_string(), os_version);
    headers.insert("X-Msh-Device-Id".to_string(), device_id);

    Ok(headers)
}

fn read_device_id(home_dir: &Path) -> String {
    let path = home_dir.join("device_id");
    std::fs::read_to_string(&path)
        .map(|s| ascii_header(s.trim()))
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string())
}

fn format_device_model() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let version = get_sys_release();
    format!("{} {} {}", os, version, arch)
}

#[cfg(target_os = "macos")]
fn get_sys_release() -> String {
    std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| get_fallback_release())
}

#[cfg(not(target_os = "macos"))]
fn get_sys_release() -> String {
    get_fallback_release()
}

fn get_fallback_release() -> String {
    std::env::consts::OS.to_string()
}

fn expand_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn ascii_header(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|c| (0x20..=0x7E).contains(&(*c as u32)))
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned.to_string()
    }
}
