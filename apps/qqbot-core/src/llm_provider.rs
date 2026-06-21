use crate::config::{AuthConfig, KimiCodeIdentity, LlmProviderConfig};
use crate::oauth::OAuthManager;
use anyhow::Result;
use async_trait::async_trait;
use brain::{BrainConfig, BrainError, ProviderFactory};
use kosong::provider::kimi::Kimi;
use kosong::provider::openai_legacy::OpenAILegacy;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default identity used when talking to the kimi-code managed endpoint.
const KIMI_CODE_PLATFORM: &str = "kimi_code_cli";

/// Factory that builds a [`kosong::ChatProvider`] based on `llm.provider_type`.
#[derive(Debug, Clone)]
pub struct QqbotProviderFactory {
    provider: LlmProviderConfig,
}

impl QqbotProviderFactory {
    pub fn new(provider: LlmProviderConfig) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl ProviderFactory for QqbotProviderFactory {
    async fn create(
        &self,
        brain_config: &BrainConfig,
    ) -> Result<Arc<dyn kosong::ChatProvider>, BrainError> {
        let token = auth_token(&self.provider)
            .await
            .map_err(|e| BrainError::Other(e.to_string()))?;
        let headers =
            identity_headers(&self.provider).map_err(|e| BrainError::Other(e.to_string()))?;
        let base_url = api_url(&self.provider);

        match &self.provider {
            LlmProviderConfig::KimiCode { .. } => {
                Ok(build_kimi_provider(brain_config, base_url, token, headers))
            }
            LlmProviderConfig::OpenAiCompatible { .. } => {
                Ok(build_openai_legacy_provider(brain_config, base_url, token))
            }
        }
    }
}

/// Resolve the bearer token for the configured provider.
async fn auth_token(provider: &LlmProviderConfig) -> Result<String> {
    match provider {
        LlmProviderConfig::OpenAiCompatible { auth, .. } => match auth {
            AuthConfig::ApiKey { api_key } => Ok(api_key.clone()),
            AuthConfig::OAuth { token_file } => {
                let oauth = crate::config::OAuthConfig {
                    provider: "oauth".to_string(),
                    token_file: token_file.clone(),
                };
                OAuthManager::new(oauth).access_token().await
            }
        },
        LlmProviderConfig::KimiCode { token_file, .. } => {
            let oauth = crate::config::OAuthConfig {
                provider: "kimi-code".to_string(),
                token_file: token_file.clone(),
            };
            OAuthManager::new(oauth).access_token().await
        }
    }
}

/// Build any extra headers required by the provider.
fn identity_headers(provider: &LlmProviderConfig) -> Result<HashMap<String, String>> {
    match provider {
        LlmProviderConfig::KimiCode { identity, .. } => build_kimi_code_identity_headers(identity),
        LlmProviderConfig::OpenAiCompatible { .. } => Ok(HashMap::new()),
    }
}

fn api_url(provider: &LlmProviderConfig) -> &str {
    match provider {
        LlmProviderConfig::OpenAiCompatible { api_url, .. } => api_url,
        LlmProviderConfig::KimiCode { api_url, .. } => api_url,
    }
}

fn build_openai_legacy_provider(
    brain_config: &BrainConfig,
    base_url: &str,
    token: String,
) -> Arc<dyn kosong::ChatProvider> {
    let mut provider = OpenAILegacy::new(&brain_config.model)
        .with_base_url(base_url)
        .with_stream(false);
    if !token.is_empty() {
        provider = provider.with_api_key(token);
    }
    Arc::new(provider)
}

fn build_kimi_provider(
    brain_config: &BrainConfig,
    base_url: &str,
    token: String,
    headers: HashMap<String, String>,
) -> Arc<dyn kosong::ChatProvider> {
    let mut provider = Kimi::new(&brain_config.model)
        .with_base_url(base_url)
        .with_api_key(token)
        .with_stream(false);
    for (name, value) in headers {
        provider = provider.with_header(name, value);
    }
    Arc::new(provider)
}

/// Build the identity headers required by the kimi-code coding endpoint.
///
/// The device id is read from `<home_dir>/device_id`, which matches
/// kimi-code's layout (`~/.kimi/device_id` or `~/.kimi-code/device_id`).
pub fn build_kimi_code_identity_headers(
    identity: &KimiCodeIdentity,
) -> Result<HashMap<String, String>> {
    let home_dir = expand_path(&identity.home_dir);

    let device_id = read_device_id(&home_dir);
    let hostname = ascii_header(std::env::var("HOSTNAME").as_deref().unwrap_or("qqbot-core"));
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
