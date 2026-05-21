use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchWebParams {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchURLParams {
    pub url: String,
}

pub struct SearchWebTool;
pub struct FetchURLTool;

impl SearchWebTool {
    pub fn new() -> Self {
        Self
    }
}

impl FetchURLTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for SearchWebTool {
    fn name(&self) -> &str {
        "SearchWeb"
    }

    fn description(&self) -> &str {
        "Search the web for information."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "SearchWeb",
            "description": "Search the web for information.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: SearchWebParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        Ok(format!(
            "Web search results for '{}' would appear here.",
            params.query
        ))
    }
}

#[async_trait]
impl Tool for FetchURLTool {
    fn name(&self) -> &str {
        "FetchURL"
    }

    fn description(&self) -> &str {
        "Fetch the content of a web page."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "FetchURL",
            "description": "Fetch the content of a web page.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" }
                },
                "required": ["url"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: FetchURLParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        let client = reqwest::Client::new();
        let resp = client
            .get(&params.url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch URL: {}", e))?;

        let text = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        Ok(text)
    }
}
