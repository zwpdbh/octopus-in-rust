use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentParams {
    pub description: String,
    #[serde(default)]
    pub prompt: String,
}

pub struct AgentTool;

impl AgentTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Launch a subagent to work on a focused task."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "Agent",
            "description": "Launch a subagent to work on a focused task in the background.",
            "parameters": {
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "Short description of the task (3-5 words)" },
                    "prompt": { "type": "string", "description": "Complete prompt for the subagent" }
                },
                "required": ["description"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: AgentParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        Ok(format!(
            "Subagent launched: {}\nThe subagent will work on this task independently and report back when done.",
            params.description
        ))
    }
}
