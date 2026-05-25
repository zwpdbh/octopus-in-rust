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
