use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub mod client;

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
pub struct McpServerInfo {
    pub status: McpServerStatus,
    pub tools: Vec<McpToolInfo>,
    pub client: Option<crate::mcp::client::McpClient>,
}

impl Clone for McpServerInfo {
    fn clone(&self) -> Self {
        Self {
            status: self.status.clone(),
            tools: self.tools.clone(),
            client: self.client.clone(),
        }
    }
}

impl std::fmt::Debug for McpServerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServerInfo")
            .field("status", &self.status)
            .field("tools", &self.tools)
            .field("client", &self.client.is_some())
            .finish()
    }
}

impl McpServerInfo {
    pub fn new(status: McpServerStatus) -> Self {
        Self {
            status,
            tools: Vec::new(),
            client: None,
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

// ============================================================================
// MCP Tool wrapper (implements Tool trait)
// ============================================================================

use async_trait::async_trait;

/// A tool proxy that delegates to an MCP server.
#[derive(Clone)]
pub struct McpTool {
    name: String,
    description: String,
    schema: serde_json::Value,
    client: crate::mcp::client::McpClient,
}

impl std::fmt::Debug for McpTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpTool")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

impl McpTool {
    pub fn new(
        name: String,
        description: String,
        schema: serde_json::Value,
        client: crate::mcp::client::McpClient,
    ) -> Self {
        Self {
            name,
            description,
            schema,
            client,
        }
    }
}

#[async_trait]
impl crate::tools::Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    async fn call(&self, arguments: serde_json::Value) -> Result<String, String> {
        match self.client.call_tool(&self.name, arguments).await {
            Ok(result) => {
                let text = result.to_text();
                if result.is_error.unwrap_or(false) {
                    Err(text)
                } else {
                    Ok(text)
                }
            }
            Err(e) => Err(format!("MCP tool error: {}", e)),
        }
    }
}
