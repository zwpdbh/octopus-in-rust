use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfigFile {
    pub onebot: OneBotConfig,
    pub bot: BotConfig,
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneBotConfig {
    pub ws_url: String,
    #[serde(default)]
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub allowed_groups: Vec<i64>,
    pub command_prefix: String,
    pub plugin_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
}

fn default_system_prompt() -> String {
    "You are a helpful assistant summarizing a QQ group conversation.".to_string()
}

impl CoreConfigFile {
    pub fn new(
        ws_url: String,
        plugin_dir: String,
        allowed_groups: Vec<i64>,
        llm: LlmConfig,
    ) -> Self {
        Self {
            onebot: OneBotConfig {
                ws_url,
                access_token: String::new(),
            },
            bot: BotConfig {
                allowed_groups,
                command_prefix: "/".to_string(),
                plugin_dir,
            },
            llm,
        }
    }

    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }
}
