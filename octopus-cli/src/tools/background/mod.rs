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

pub struct TaskOutputTool {
    bg_manager: crate::background::BackgroundTaskManager,
}

pub struct TaskStopTool {
    bg_manager: crate::background::BackgroundTaskManager,
}

impl TaskOutputTool {
    pub fn new(bg_manager: crate::background::BackgroundTaskManager) -> Self {
        Self { bg_manager }
    }
}

impl TaskStopTool {
    pub fn new(bg_manager: crate::background::BackgroundTaskManager) -> Self {
        Self { bg_manager }
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

        let (status, output) = self
            .bg_manager
            .get_output(&params.task_id)
            .await
            .ok_or_else(|| format!("Task '{}' not found", params.task_id))?;

        let status_str = match status {
            crate::background::TaskStatus::Running => "running".to_string(),
            crate::background::TaskStatus::Completed(code) => {
                format!("completed (exit code {})", code)
            }
            crate::background::TaskStatus::Failed(ref e) => format!("failed: {}", e),
            crate::background::TaskStatus::Killed => "killed".to_string(),
        };

        if params.block {
            // If block=true, poll until the task is no longer running
            let mut current_status = status;
            let mut current_output = output;
            while matches!(current_status, crate::background::TaskStatus::Running) {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                let result = self
                    .bg_manager
                    .get_output(&params.task_id)
                    .await
                    .ok_or_else(|| format!("Task '{}' disappeared", params.task_id))?;
                current_status = result.0;
                current_output = result.1;
            }
            let final_status = match current_status {
                crate::background::TaskStatus::Completed(code) => {
                    format!("completed (exit code {})", code)
                }
                crate::background::TaskStatus::Failed(ref e) => format!("failed: {}", e),
                crate::background::TaskStatus::Killed => "killed".to_string(),
                _ => "unknown".to_string(),
            };
            return Ok(format!(
                "Status: {}\n\nOutput:\n{}",
                final_status, current_output
            ));
        }

        Ok(format!("Status: {}\n\nOutput:\n{}", status_str, output))
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

        self.bg_manager
            .stop(&params.task_id)
            .await
            .map_err(|e| format!("Failed to stop task: {}", e))?;

        Ok(format!(
            "Task {} stopped. Reason: {}",
            params.task_id, params.reason
        ))
    }
}
