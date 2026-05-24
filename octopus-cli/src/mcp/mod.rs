use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Load MCP config from the global config file (`~/.kimi/mcp.json`).
pub fn load_mcp_config() -> McpConfig {
    let path = crate::share::get_share_dir().join("mcp.json");
    if !path.exists() {
        return McpConfig::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(e) => {
            tracing::warn!("Failed to read MCP config from {}: {}", path.display(), e);
            McpConfig::default()
        }
    }
}

/// Save MCP config to the global config file.
pub fn save_mcp_config(config: &McpConfig) {
    let path = crate::share::get_share_dir().join("mcp.json");
    match serde_json::to_string_pretty(config) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                tracing::warn!("Failed to write MCP config to {}: {}", path.display(), e);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to serialize MCP config: {}", e);
        }
    }
}

/// Top-level MCP config structure matching the `mcp.json` schema.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(rename = "mcpServers", default)]
    pub servers: HashMap<String, McpServerConfig>,
}

/// Per-server MCP configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Transport type: "stdio", "http", or "sse".
    #[serde(default = "default_transport")]
    pub transport: String,

    // stdio transport
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,

    // HTTP/SSE transport
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,

    // Auth
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
}

fn default_transport() -> String {
    "stdio".to_string()
}

/// Runtime state for an MCP server.
#[derive(Debug, Clone)]
pub struct McpServerInfo {
    pub status: McpServerStatus,
    pub tools: Vec<McpToolInfo>,
}

impl McpServerInfo {
    pub fn new(status: McpServerStatus) -> Self {
        Self {
            status,
            tools: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerStatus {
    Pending,
    Connecting,
    Connected,
    Failed,
    Unauthorized,
}

impl McpServerStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            McpServerStatus::Pending => "pending",
            McpServerStatus::Connecting => "connecting",
            McpServerStatus::Connected => "connected",
            McpServerStatus::Failed => "failed",
            McpServerStatus::Unauthorized => "unauthorized",
        }
    }
}

/// Information about a single tool exposed by an MCP server.
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}
