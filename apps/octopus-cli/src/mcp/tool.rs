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
impl kosong::tooling::CallableTool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        // McpTool stores the full wrapper ({name, description, parameters}).
        // CallableTool::parameters() must return only the parameters JSON Schema.
        self.schema
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}))
    }

    async fn call_raw(&self, arguments: serde_json::Value) -> kosong::tooling::ToolReturnValue {
        match self.client.call_tool(&self.name, arguments).await {
            Ok(result) => {
                let text = result.to_text();
                if result.is_error.unwrap_or(false) {
                    kosong::tooling::ToolReturnValue::error(text)
                } else {
                    kosong::tooling::ToolReturnValue::ok(text)
                }
            }
            Err(e) => kosong::tooling::ToolReturnValue::error(format!("MCP tool error: {}", e)),
        }
    }
}
