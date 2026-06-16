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
    #[serde(default)]
    pub bot_qq: i64,
    #[serde(default)]
    pub bot_aliases: Vec<String>,
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

impl CoreConfigFile {
    pub fn new(
        ws_url: String,
        plugin_dir: String,
        allowed_groups: Vec<i64>,
        bot_qq: i64,
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
                bot_qq,
                bot_aliases: Vec::new(),
            },
            llm,
        }
    }

    pub fn default_llm_config(api_key: String) -> LlmConfig {
        LlmConfig {
            model: "moonshot-v1-8k".to_string(),
            system_prompt: default_system_prompt(),
            provider: LlmProviderConfig::OpenAiCompatible {
                api_url: "https://api.moonshot.ai/v1/chat/completions".to_string(),
                auth: AuthConfig::ApiKey { api_key },
            },
        }
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }
}
