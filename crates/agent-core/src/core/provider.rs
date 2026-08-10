use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use llm_provider::provider::kimi::Kimi;
use llm_provider::provider::openai_legacy::OpenAILegacy;
use llm_provider::provider::openai_responses::OpenAIResponses;
use serde::{Deserialize, Serialize};

use crate::core::config::BrainConfig;
use crate::core::errors::BrainError;
use crate::core::oauth::{OAuthConfig, OAuthManager};

/// How a Brain authenticates and connects to an LLM backend.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "provider_type", rename_all = "snake_case")]
pub enum ProviderType {
    /// API-key provider (OpenAI-compatible legacy chat completions or
    /// OpenAI Responses API).
    ApiBased {
        /// Which API-based protocol to use.
        #[serde(default)]
        protocol: ApiProtocol,
        /// API key or access token sent in the `Authorization` header.
        api_key: String,
        /// Optional reasoning key for providers that support it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_key: Option<String>,
    },

    /// Subscription/OAuth provider that reads a bearer token from a file,
    /// refreshes it when needed, and attaches identity headers.
    SubscriptionBased {
        /// Which subscription protocol to use.
        #[serde(default)]
        protocol: SubscriptionProtocol,
        /// Path to the JSON file containing the OAuth access token.
        token_file: PathBuf,
        /// Identity headers required by the subscription endpoint.
        #[serde(default)]
        identity: ProviderIdentity,
        /// OAuth configuration used to refresh the access token.
        #[serde(default = "default_oauth_config")]
        oauth: OAuthConfig,
    },
}

impl ProviderType {
    /// Return a label for logging that does not expose secrets.
    pub fn label(&self) -> &'static str {
        match self {
            ProviderType::ApiBased { .. } => "api_based",
            ProviderType::SubscriptionBased { .. } => "subscription_based",
        }
    }

    /// Whether this provider uses a static API key.
    pub fn is_api_based(&self) -> bool {
        matches!(self, ProviderType::ApiBased { .. })
    }

    /// Whether this provider uses OAuth / subscription auth.
    pub fn is_subscription_based(&self) -> bool {
        matches!(self, ProviderType::SubscriptionBased { .. })
    }
}

impl fmt::Display for ProviderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// API-based provider protocol.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProtocol {
    /// OpenAI-compatible legacy chat completions endpoint.
    #[default]
    OpenAiLegacy,
    /// OpenAI Responses API endpoint.
    OpenAiResponses,
}

/// Subscription-based provider protocol.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionProtocol {
    /// Kimi Code managed endpoint.
    #[default]
    Kimi,
}

/// Identity headers required by subscription-based providers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderIdentity {
    /// Platform identifier sent in `X-Msh-Platform`.
    #[serde(default = "default_kimi_code_platform")]
    pub platform: String,

    /// Version sent in `X-Msh-Version` and used in `User-Agent`.
    #[serde(default = "default_kimi_code_version")]
    pub version: String,

    /// Product name used in `User-Agent` (`{product}/{version}`).
    #[serde(default = "default_kimi_code_product")]
    pub user_agent_product: String,

    /// Home directory used to locate the `device_id` file.
    #[serde(default = "default_kimi_code_home")]
    pub home_dir: PathBuf,
}

impl ProviderIdentity {
    /// Return a reasonable default for the Kimi Code managed endpoint.
    pub fn kimi_code_default() -> Self {
        Self {
            platform: default_kimi_code_platform(),
            version: default_kimi_code_version(),
            user_agent_product: default_kimi_code_product(),
            home_dir: default_kimi_code_home(),
        }
    }
}

impl Default for ProviderIdentity {
    fn default() -> Self {
        Self::kimi_code_default()
    }
}

fn default_kimi_code_platform() -> String {
    "kimi_code_cli".to_string()
}

fn default_kimi_code_version() -> String {
    "1.0.0".to_string()
}

fn default_kimi_code_product() -> String {
    "kimi-code-cli".to_string()
}

fn default_kimi_code_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kimi")
}

fn default_oauth_config() -> OAuthConfig {
    OAuthConfig::kimi_code()
}

/// Builds a [`llm_provider::ChatProvider`] for a Brain instance.
///
/// Applications implement this trait to control how the LLM provider is
/// constructed and reconstructed (e.g. after an OAuth token refresh).
#[async_trait]
pub trait ProviderFactory: Send + Sync {
    async fn create(
        &self,
        config: &BrainConfig,
    ) -> Result<Arc<dyn llm_provider::ChatProvider>, BrainError>;
}

/// Default factory that builds a provider from [`ProviderType`] configured on
/// [`BrainConfig`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultProviderFactory;

#[async_trait]
impl ProviderFactory for DefaultProviderFactory {
    async fn create(
        &self,
        config: &BrainConfig,
    ) -> Result<Arc<dyn llm_provider::ChatProvider>, BrainError> {
        match &config.provider_type {
            ProviderType::ApiBased {
                protocol,
                api_key,
                reasoning_key,
            } => Ok(build_api_provider(
                config,
                protocol,
                api_key,
                reasoning_key.as_deref(),
            )?),
            ProviderType::SubscriptionBased {
                protocol,
                token_file,
                identity,
                oauth,
            } => Ok(
                build_subscription_provider(config, protocol, token_file, identity, oauth).await?,
            ),
        }
    }
}

fn build_api_provider(
    config: &BrainConfig,
    protocol: &ApiProtocol,
    api_key: &str,
    reasoning_key: Option<&str>,
) -> Result<Arc<dyn llm_provider::ChatProvider>, BrainError> {
    if config.base_url.is_empty() || config.model.is_empty() {
        return Err(BrainError::NoProvider);
    }

    let api_key_empty = api_key.is_empty();

    // NOTE: Arbitrary custom headers are not supported here because the
    // `llm-provider` OpenAI builders (`OpenAILegacy`, `OpenAIResponses`) do
    // not expose a `with_header` API. If that changes, `BrainConfig` should
    // gain a `custom_headers` field and pass it through below.
    match protocol {
        ApiProtocol::OpenAiLegacy => {
            let mut provider = OpenAILegacy::new(&config.model)
                .with_base_url(&config.base_url)
                .with_stream(false);
            if !api_key_empty {
                provider = provider.with_api_key(api_key);
            }
            if let Some(key) = reasoning_key {
                provider = provider.with_reasoning_key(key);
            }
            Ok(Arc::new(provider))
        }
        ApiProtocol::OpenAiResponses => {
            let mut provider = OpenAIResponses::new(&config.model).with_base_url(&config.base_url);
            if !api_key_empty {
                provider = provider.with_api_key(api_key);
            }
            Ok(Arc::new(provider))
        }
    }
}

async fn build_subscription_provider(
    config: &BrainConfig,
    protocol: &SubscriptionProtocol,
    token_file: &Path,
    identity: &ProviderIdentity,
    oauth: &OAuthConfig,
) -> Result<Arc<dyn llm_provider::ChatProvider>, BrainError> {
    let manager = OAuthManager::new(oauth.clone(), token_file);
    let token = manager
        .access_token()
        .await
        .map_err(|e| BrainError::Other(format!("failed to resolve subscription token: {e}")))?;

    let headers = build_identity_headers(identity)
        .await
        .map_err(|e| BrainError::Other(format!("failed to build identity headers: {e}")))?;

    match protocol {
        SubscriptionProtocol::Kimi => {
            let base_url = if config.base_url.is_empty() {
                "https://api.kimi.com/coding/v1"
            } else {
                &config.base_url
            };

            let mut provider = Kimi::new(&config.model)
                .with_base_url(base_url)
                .with_api_key(token)
                .with_stream(false);

            for (name, value) in headers {
                provider = provider.with_header(name, value);
            }

            Ok(Arc::new(provider))
        }
    }
}

async fn build_identity_headers(
    identity: &ProviderIdentity,
) -> anyhow::Result<HashMap<String, String>> {
    let device_id = tokio::fs::read_to_string(identity.home_dir.join("device_id"))
        .await
        .map(|s| ascii_header(s.trim()))
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

    let hostname = ascii_header(std::env::var("HOSTNAME").as_deref().unwrap_or("agent-core"));
    let device_model = ascii_header(&format_device_model());
    let os_version = ascii_header(get_sys_release().as_str());

    let mut headers = HashMap::new();
    headers.insert(
        "User-Agent".to_string(),
        format!("{}/{}", identity.user_agent_product, identity.version),
    );
    headers.insert("X-Msh-Platform".to_string(), identity.platform.clone());
    headers.insert("X-Msh-Version".to_string(), identity.version.clone());
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
