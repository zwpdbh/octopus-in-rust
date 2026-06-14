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
        if self.llm.api_url.is_empty() {
            anyhow::bail!("llm.api_url must not be empty");
        }
        let has_oauth = self.llm.oauth.is_some();
        if self.llm.api_key.is_empty() && !has_oauth {
            anyhow::bail!("llm.api_key must not be empty unless llm.oauth is configured");
        }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_url: String,
    #[serde(default)]
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default)]
    pub oauth: Option<crate::oauth::OAuthConfig>,
}

fn default_system_prompt() -> String {
    "You are a helpful assistant summarizing a QQ group conversation.".to_string()
}
