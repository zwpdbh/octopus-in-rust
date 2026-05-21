use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutputParams {
    pub task_id: String,
    #[serde(default)]
    pub block: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStopParams {
    pub task_id: String,
    #[serde(default)]
    pub reason: String,
}

pub struct TaskOutputTool;
pub struct TaskStopTool;

impl TaskOutputTool {
    pub fn new() -> Self {
        Self
    }
}

impl TaskStopTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        "TaskOutput"
    }

    fn description(&self) -> &str {
        "Get the output of a background task."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "TaskOutput",
            "description": "Get the output of a background task.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task ID" },
                    "block": { "type": "boolean", "default": false, "description": "Wait for completion" }
                },
                "required": ["task_id"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: TaskOutputParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        Ok(format!(
            "Output for task {} would appear here.",
            params.task_id
        ))
    }
}

#[async_trait]
impl Tool for TaskStopTool {
    fn name(&self) -> &str {
        "TaskStop"
    }

    fn description(&self) -> &str {
        "Stop a background task."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "TaskStop",
            "description": "Stop a background task.",
            "parameters": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "Task ID" },
                    "reason": { "type": "string", "default": "Stopped by TaskStop" }
                },
                "required": ["task_id"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: TaskStopParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        Ok(format!(
            "Task {} stopped. Reason: {}",
            params.task_id, params.reason
        ))
    }
}
