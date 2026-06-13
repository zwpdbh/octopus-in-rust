use serde::{Deserialize, Serialize};
use std::path::Path;

/// Bot configuration loaded from `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub onebot: OneBotConfig,
    pub napcat: NapcatConfig,
    pub bot: BotConfig,
    pub llm: LlmConfig,
}

impl Config {
    /// Load configuration from a TOML file.
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
        if self.llm.api_key.is_empty() {
            anyhow::bail!("llm.api_key must not be empty");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneBotConfig {
    /// WebSocket URL exposed by NapCatQQ for OneBot events / API.
    pub ws_url: String,
    /// Optional access token for OneBot authentication.
    #[serde(default)]
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NapcatConfig {
    /// Directory where NapCatQQ is installed.
    pub dir: String,
    /// Command used to launch NapCatQQ, relative to `dir`.
    pub launch_command: String,
    /// Working directory for NapCatQQ runtime data (logs, caches, etc.).
    #[serde(default = "default_napcat_data_dir")]
    pub data_dir: String,
    /// Arguments passed to the NapCatQQ launcher.
    #[serde(default)]
    pub launch_args: Vec<String>,
}

fn default_napcat_data_dir() -> String {
    "napcat-data".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    /// Only respond in these group IDs. Empty means all groups.
    #[serde(default)]
    pub allowed_groups: Vec<i64>,
    /// Prefix for bot commands (e.g. "/").
    pub command_prefix: String,
    /// Time window in seconds for conversation summarization.
    pub summary_window_secs: u64,
    /// Maximum number of messages to keep per group buffer.
    pub max_buffer_size: usize,
}

impl BotConfig {
    pub fn is_group_allowed(&self, group_id: i64) -> bool {
        self.allowed_groups.is_empty() || self.allowed_groups.contains(&group_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// OpenAI-compatible chat completion endpoint.
    pub api_url: String,
    /// API key for the LLM endpoint.
    pub api_key: String,
    /// Model name to use.
    pub model: String,
    /// System prompt used when generating summaries.
    pub system_prompt: String,
    /// Maximum number of buffered messages to send as context.
    pub max_context_messages: usize,
}
