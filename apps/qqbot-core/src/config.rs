use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub onebot: OneBotConfig,
    pub bot: BotConfig,
    pub llm: LlmConfig,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.onebot.ws_url.is_empty() {
            anyhow::bail!("onebot.ws_url must not be empty");
        }
        self.llm.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneBotConfig {
    pub ws_url: String,
    #[serde(default)]
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    #[serde(default)]
    pub allowed_groups: Vec<i64>,
    #[serde(default = "default_command_prefix")]
    pub command_prefix: String,
    #[serde(default = "default_plugin_dir")]
    pub plugin_dir: String,
    #[serde(default)]
    pub bot_qq: i64,
    #[serde(default)]
    pub bot_aliases: Vec<String>,
}

fn default_command_prefix() -> String {
    "/".to_string()
}

fn default_plugin_dir() -> String {
    "plugins".to_string()
}

impl BotConfig {
    pub fn is_group_allowed(&self, group_id: i64) -> bool {
        self.allowed_groups.is_empty() || self.allowed_groups.contains(&group_id)
    }
}

/// LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub model: String,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(flatten)]
    pub provider: LlmProviderConfig,
}

impl LlmConfig {
    pub fn api_url(&self) -> &str {
        match &self.provider {
            LlmProviderConfig::OpenAiCompatible { api_url, .. } => api_url,
            LlmProviderConfig::KimiCode { api_url, .. } => api_url,
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.model.is_empty() {
            anyhow::bail!("llm.model must not be empty");
        }
        match &self.provider {
            LlmProviderConfig::OpenAiCompatible { api_url, auth } => {
                if api_url.is_empty() {
                    anyhow::bail!("llm.api_url must not be empty");
                }
                auth.validate()?;
            }
            LlmProviderConfig::KimiCode {
                api_url,
                token_file,
                ..
            } => {
                if api_url.is_empty() {
                    anyhow::bail!("llm.api_url must not be empty");
                }
                if token_file.is_empty() {
                    anyhow::bail!("llm.token_file must not be empty for kimi_code provider");
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider_type", rename_all = "snake_case")]
pub enum LlmProviderConfig {
    /// Generic OpenAI-compatible endpoint (Moonshot, DeepSeek, OpenAI, etc.).
    OpenAiCompatible {
        api_url: String,
        #[serde(flatten)]
        auth: AuthConfig,
    },
    /// Kimi Code managed endpoint using OAuth device-flow credentials.
    KimiCode {
        api_url: String,
        token_file: String,
        #[serde(flatten)]
        identity: KimiCodeIdentity,
    },
}

/// Authentication method for an OpenAI-compatible provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "auth_type", rename_all = "snake_case")]
pub enum AuthConfig {
    ApiKey { api_key: String },
    OAuth { token_file: String },
}

impl AuthConfig {
    fn validate(&self) -> anyhow::Result<()> {
        match self {
            AuthConfig::ApiKey { api_key } => {
                if api_key.is_empty() {
                    anyhow::bail!("llm.api_key must not be empty when auth_type = 'api_key'");
                }
            }
            AuthConfig::OAuth { token_file } => {
                if token_file.is_empty() {
                    anyhow::bail!("llm.token_file must not be empty when auth_type = 'oauth'");
                }
            }
        }
        Ok(())
    }
}

/// Identity headers required by the kimi-code coding endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiCodeIdentity {
    #[serde(default = "default_kimi_code_home")]
    pub home_dir: String,
    #[serde(default = "default_kimi_code_version")]
    pub version: String,
    #[serde(default = "default_kimi_code_product")]
    pub user_agent_product: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub provider: String,
    pub token_file: String,
}

fn default_system_prompt() -> String {
    "You are a helpful assistant summarizing a QQ group conversation.".to_string()
}

fn default_kimi_code_home() -> String {
    "~/.kimi".to_string()
}

fn default_kimi_code_version() -> String {
    "0.1.1".to_string()
}

fn default_kimi_code_product() -> String {
    "kimi-code-cli".to_string()
}
