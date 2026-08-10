use crate::core_config::{AuthConfig, CoreConfigFile, LlmConfig, LlmProviderConfig};
use anyhow::Result;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use llm_provider::{ChatProvider, ContentPart, Message, Role, StreamedMessagePart, Tool};

pub async fn ask(
    data_dir: &Path,
    prompt: &str,
    model_override: Option<&str>,
    base_url_override: Option<&str>,
) -> Result<()> {
    let config = CoreConfigFile::from_file(data_dir.join("config.toml"))?;
    let llm = &config.llm;

    println!("=== qqbot llm ask ===\n");

    if llm.model.is_empty() {
        println!("[fail] llm.model is not configured");
        return Ok(());
    }

    let provider = build_chat_provider(llm, base_url_override, model_override, false).await?;
    let model = model_override.unwrap_or(&llm.model);

    println!("Model:    {model}");
    println!("Prompt:   {prompt}\n");

    let history = vec![Message {
        role: Role::User,
        name: None,
        content: vec![ContentPart::Text {
            text: prompt.to_string(),
        }],
        tool_calls: None,
        tool_call_id: None,
        partial: None,
    }];

    let result = llm_provider::generate(
        provider.as_ref(),
        &llm.system_prompt,
        &[] as &[Tool],
        &history,
        None,
        None,
    )
    .await?;

    println!("Reply:");
    println!("{}", extract_text(&result.message));

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

    if llm.model.is_empty() {
        println!("[fail] llm.model is not configured");
        return Ok(());
    }

    let provider = build_chat_provider(llm, base_url_override, model_override, true).await?;
    let model = model_override.unwrap_or(&llm.model);

    println!("[config] model:    {model}");
    println!("[config] prompt:   {prompt}\n");
    println!("[streaming] waiting for first chunk...\n");
    println!("--- reply ---");

    let history = vec![Message {
        role: Role::User,
        name: None,
        content: vec![ContentPart::Text {
            text: prompt.to_string(),
        }],
        tool_calls: None,
        tool_call_id: None,
        partial: None,
    }];

    let mut started = false;
    let mut on_part = |part: StreamedMessagePart| {
        if let StreamedMessagePart::Content(ContentPart::Text { text }) = part {
            if !text.is_empty() {
                if !started {
                    started = true;
                    println!("[first chunk received]");
                }
                print!("{text}");
                let _ = stdout().flush();
            }
        }
    };

    let _ = llm_provider::generate(
        provider.as_ref(),
        &llm.system_prompt,
        &[] as &[Tool],
        &history,
        Some(&mut on_part),
        None,
    )
    .await?;

    println!("\n--- end ---");
    println!("[done]");
    Ok(())
}

/// Verify provider authentication by listing available models.
///
/// Uses `ChatProvider::list_models` to confirm the API key / OAuth token and
/// base URL are valid, and checks that the configured model exists.
pub async fn test(data_dir: &Path, base_url_override: Option<&str>) -> Result<()> {
    let config = CoreConfigFile::from_file(data_dir.join("config.toml"))?;
    let llm = &config.llm;

    println!("=== qqbot llm test ===\n");

    if llm.model.is_empty() {
        println!("[fail] llm.model is not configured");
        return Ok(());
    }

    let configured_url = match &llm.provider {
        LlmProviderConfig::OpenAiCompatible { api_url, .. } => api_url.as_str(),
        LlmProviderConfig::KimiCode { api_url, .. } => api_url.as_str(),
    };
    let api_url = base_url_override.unwrap_or(configured_url);

    let provider = build_chat_provider(llm, base_url_override, None, false).await?;

    println!("[ok]   provider: {}", provider.name());
    println!("[ok]   endpoint: {}", api_url);
    println!("[ok]   model:    {}", llm.model);

    let ids = match provider.list_models().await {
        Ok(ids) => ids,
        Err(e) => {
            println!();
            println!("[fail] Failed to list models: {e}");
            println!(
                "       This usually means the API key is invalid or expired, or the endpoint is unreachable."
            );
            println!(
                "       Update llm.auth in {} and run `qqbot restart`.",
                data_dir.join("config.toml").display()
            );
            return Ok(());
        }
    };

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

async fn build_chat_provider(
    llm: &LlmConfig,
    base_url_override: Option<&str>,
    model_override: Option<&str>,
    stream: bool,
) -> Result<Arc<dyn ChatProvider>> {
    let model = model_override.unwrap_or(&llm.model).to_string();

    match &llm.provider {
        LlmProviderConfig::OpenAiCompatible { api_url, auth } => {
            let token = resolve_auth(auth).await?;
            let base_url = base_url_override.unwrap_or(api_url);
            let provider = llm_provider::provider::openai_legacy::OpenAILegacy::new(&model)
                .with_base_url(base_url)
                .with_api_key(&token)
                .with_stream(stream);
            Ok(Arc::new(provider))
        }
        LlmProviderConfig::KimiCode {
            api_url,
            token_file,
            identity,
        } => {
            let token = resolve_oauth_token(token_file).await?;
            let base_url = base_url_override.unwrap_or(api_url);
            let provider_identity = agent_core::ProviderIdentity {
                platform: "kimi_code_cli".to_string(),
                version: identity.version.clone(),
                user_agent_product: identity.user_agent_product.clone(),
                home_dir: expand_path(&identity.home_dir),
            };
            let headers =
                agent_core::core::provider::build_identity_headers(&provider_identity).await?;
            let mut provider = llm_provider::provider::kimi::Kimi::new(&model)
                .with_base_url(base_url)
                .with_api_key(&token)
                .with_stream(stream);
            for (name, value) in headers {
                provider = provider.with_header(name, value);
            }
            Ok(Arc::new(provider))
        }
    }
}

fn extract_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|part| {
            if let ContentPart::Text { text } = part {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn expand_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}
