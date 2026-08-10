//! LLM provider factory supporting both OpenAI-compatible APIs and kimi-code.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_core::{BrainConfig, BrainError, ProviderFactory};
use anyhow::Context;
use async_trait::async_trait;
use futures::StreamExt;
use llm_provider::provider::kimi::Kimi;
use llm_provider::provider::openai_legacy::OpenAILegacy;
use serde::Deserialize;

/// Which LLM backend to use, with provider-specific credentials.
#[derive(Clone)]
pub enum ProviderType {
    /// Generic OpenAI-compatible endpoint (OpenAI, Moonshot, etc.).
    OpenAiCompatible { api_key: String },

    /// Kimi Code managed endpoint using OAuth token + identity headers.
    KimiCode { token_file: PathBuf },
}

impl ProviderType {
    /// Return the provider label used in logs and configuration.
    pub fn label(&self) -> &'static str {
        match self {
            ProviderType::OpenAiCompatible { .. } => "openai_compatible",
            ProviderType::KimiCode { .. } => "kimi_code",
        }
    }

    /// Borrow the API key, if this is an OpenAI-compatible provider.
    pub fn api_key(&self) -> Option<&str> {
        match self {
            ProviderType::OpenAiCompatible { api_key } => Some(api_key.as_str()),
            ProviderType::KimiCode { .. } => None,
        }
    }

    /// Borrow the OAuth token file path, if this is a Kimi Code provider.
    pub fn token_file(&self) -> Option<&Path> {
        match self {
            ProviderType::OpenAiCompatible { .. } => None,
            ProviderType::KimiCode { token_file } => Some(token_file.as_path()),
        }
    }
}

impl fmt::Debug for ProviderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderType::OpenAiCompatible { api_key } => {
                let status = if api_key.is_empty() { "missing" } else { "set" };
                f.debug_struct("OpenAiCompatible")
                    .field("api_key", &status)
                    .finish()
            }
            ProviderType::KimiCode { token_file } => f
                .debug_struct("KimiCode")
                .field("token_file", &token_file.display().to_string())
                .finish(),
        }
    }
}

impl fmt::Display for ProviderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Builds the configured LLM provider for the Q&A agent.
#[derive(Clone, Debug)]
pub struct FafcnProviderFactory {
    provider_type: ProviderType,
}

impl FafcnProviderFactory {
    pub fn new(provider_type: ProviderType) -> Self {
        Self { provider_type }
    }
}

#[async_trait]
impl ProviderFactory for FafcnProviderFactory {
    async fn create(
        &self,
        brain_config: &BrainConfig,
    ) -> Result<Arc<dyn llm_provider::ChatProvider>, BrainError> {
        match &self.provider_type {
            ProviderType::OpenAiCompatible { api_key } => {
                build_openai_legacy(brain_config, api_key).map_err(error_to_brain_error)
            }
            ProviderType::KimiCode { token_file } => {
                build_kimi_code(brain_config, token_file).map_err(error_to_brain_error)
            }
        }
    }
}

fn error_to_brain_error(err: crate::error::Error) -> BrainError {
    match err {
        crate::error::Error::Agent(be) => be,
        _ => BrainError::Other(err.to_string()),
    }
}

fn build_openai_legacy(
    brain_config: &BrainConfig,
    api_key: &str,
) -> crate::Result<Arc<dyn llm_provider::ChatProvider>> {
    if brain_config.base_url.is_empty() || brain_config.model.is_empty() {
        return Err(crate::error::Error::Agent(BrainError::NoProvider));
    }

    let mut provider = OpenAILegacy::new(&brain_config.model)
        .with_base_url(&brain_config.base_url)
        .with_stream(false);

    if !api_key.is_empty() {
        provider = provider.with_api_key(api_key);
    }

    Ok(Arc::new(provider))
}

fn build_kimi_code(
    brain_config: &BrainConfig,
    token_file: &Path,
) -> crate::Result<Arc<dyn llm_provider::ChatProvider>> {
    let token = read_access_token(token_file)?;
    let headers = kimi_code_identity_headers(token_file)?;

    let base_url = if brain_config.base_url.is_empty() {
        "https://api.kimi.com/coding/v1"
    } else {
        &brain_config.base_url
    };

    let mut provider = Kimi::new(&brain_config.model)
        .with_base_url(base_url)
        .with_api_key(token)
        .with_stream(false);

    for (name, value) in headers {
        provider = provider.with_header(name, value);
    }

    Ok(Arc::new(provider))
}

#[derive(Deserialize)]
struct OAuthToken {
    access_token: String,
}

fn read_access_token(token_file: &Path) -> crate::Result<String> {
    let contents = std::fs::read_to_string(token_file)
        .with_context(|| format!("failed to read token file {}", token_file.display()))?;
    let token: OAuthToken = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse token file {}", token_file.display()))?;
    Ok(token.access_token)
}

fn kimi_code_identity_headers(token_file: &Path) -> crate::Result<HashMap<String, String>> {
    // The device id lives next to the credentials directory, e.g.
    // ~/.kimi/credentials/kimi-code.json -> ~/.kimi/device_id
    let device_id_file = token_file
        .parent()
        .and_then(Path::parent)
        .map(|p| p.join("device_id"))
        .unwrap_or_else(|| PathBuf::from("device_id"));

    let device_id = std::fs::read_to_string(&device_id_file)
        .map(|s| ascii_header(s.trim()))
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

    let hostname = ascii_header(
        std::env::var("HOSTNAME")
            .as_deref()
            .unwrap_or("fafcn-server"),
    );
    let device_model = ascii_header(&format_device_model());
    let os_version = ascii_header(get_sys_release().as_str());

    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), "kimi-code-cli/1.0.0".to_string());
    headers.insert("X-Msh-Platform".to_string(), "kimi_code_cli".to_string());
    headers.insert("X-Msh-Version".to_string(), "1.0.0".to_string());
    headers.insert("X-Msh-Device-Name".to_string(), hostname);
    headers.insert("X-Msh-Device-Model".to_string(), device_model);
    headers.insert("X-Msh-Os-Version".to_string(), os_version);
    headers.insert("X-Msh-Device-Id".to_string(), device_id);

    Ok(headers)
}

fn format_device_model() -> String {
    format!(
        "{} {} {}",
        std::env::consts::OS,
        get_sys_release(),
        std::env::consts::ARCH
    )
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
        .unwrap_or_else(|| std::env::consts::OS.to_string())
}

#[cfg(not(target_os = "macos"))]
fn get_sys_release() -> String {
    std::env::consts::OS.to_string()
}

/// Verify that the configured provider can authenticate and generate a tiny
/// response. This is intended for health checks: it costs a small number of
/// tokens, but proves the API key / OAuth token and base URL are working.
pub async fn verify_provider_auth(config: &crate::handlers::qa::QaConfig) -> crate::Result<String> {
    use llm_provider::{ContentPart, Message, Role, StreamedMessagePart, Tool};

    let factory = FafcnProviderFactory::new(config.provider_type.clone());
    let brain_config = BrainConfig {
        system_prompt: config.system_prompt.clone(),
        base_url: config.base_url.clone(),
        api_key: config.provider_type.api_key().unwrap_or("").to_string(),
        model: config.model.clone(),
        max_steps_per_turn: config.max_steps_per_turn,
        tool_sources: vec![],
        ..Default::default()
    };

    let provider = factory
        .create(&brain_config)
        .await
        .map_err(crate::error::Error::Agent)?;

    let history = vec![Message {
        role: Role::User,
        name: None,
        content: vec![ContentPart::Text {
            text: "ping".to_string(),
        }],
        tool_calls: None,
        tool_call_id: None,
        partial: None,
    }];

    let system = "You are a helpful assistant. Reply with exactly the word pong.";
    let streamed = provider
        .generate(system, &[] as &[Tool], &history)
        .await
        .map_err(|e| {
            crate::error::Error::Internal(format!("provider auth / connectivity check failed: {e}"))
        })?;

    let mut reply = String::new();
    let mut stream = streamed.stream;
    while let Some(part) = stream.next().await {
        if let StreamedMessagePart::Content(ContentPart::Text { text }) = part {
            reply.push_str(&text);
        }
    }

    Ok(reply)
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
