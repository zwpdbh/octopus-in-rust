use std::path::Path;

use agent_core::ProviderType;
use serde::{Deserialize, Serialize};

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
    pub api_url: String,
    #[serde(flatten)]
    pub provider: ProviderType,
}

impl LlmConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.model.is_empty() {
            anyhow::bail!("llm.model must not be empty");
        }
        if self.api_url.is_empty() {
            anyhow::bail!("llm.api_url must not be empty");
        }
        match &self.provider {
            ProviderType::ApiBased { api_key, .. } => {
                if api_key.is_empty() {
                    anyhow::bail!("llm.api_key must not be empty when provider_type = 'api_based'");
                }
            }
            ProviderType::SubscriptionBased { token_file, .. } => {
                if token_file.as_os_str().is_empty() {
                    anyhow::bail!(
                        "llm.token_file must not be empty when provider_type = 'subscription_based'"
                    );
                }
            }
        }
        Ok(())
    }
}

fn default_system_prompt() -> String {
    "You are a helpful assistant summarizing a QQ group conversation.".to_string()
}
