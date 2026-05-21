use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkParams {
    pub thought: String,
}

pub struct ThinkTool;

impl ThinkTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ThinkTool {
    fn name(&self) -> &str {
        "Think"
    }

    fn description(&self) -> &str {
        "Think through a problem step by step."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "Think",
            "description": "Think through a problem step by step. Use this to reason before taking action.",
            "parameters": {
                "type": "object",
                "properties": {
                    "thought": { "type": "string", "description": "Your thought process" }
                },
                "required": ["thought"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: ThinkParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        Ok(format!("Thought recorded: {}", params.thought))
    }
}
