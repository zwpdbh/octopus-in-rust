use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub qq: QqConfig,
    pub napcat: NapcatConfig,
    pub core: CoreConfig,
    pub llm: LlmConfig,
}

impl Config {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QqConfig {
    pub account: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NapcatConfig {
    pub dir: String,
    pub launcher: String,
    pub data_dir: String,
    pub ws_port: u16,
    pub webui_port: u16,
}

impl Default for NapcatConfig {
    fn default() -> Self {
        Self {
            dir: "./napcat".to_string(),
            launcher: "napcat.sh".to_string(),
            data_dir: "./data/napcat".to_string(),
            ws_port: 3001,
            webui_port: 6099,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    pub binary: String,
    pub plugin_dir: String,
    pub config_path: String,
    pub allowed_groups: Vec<i64>,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            binary: "./qqbot-core".to_string(),
            plugin_dir: "./plugins".to_string(),
            config_path: "./data/config.toml".to_string(),
            allowed_groups: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_key: String,
    pub api_url: String,
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_url: "https://api.moonshot.cn/v1/chat/completions".to_string(),
            model: "moonshot-v1-8k".to_string(),
        }
    }
}
