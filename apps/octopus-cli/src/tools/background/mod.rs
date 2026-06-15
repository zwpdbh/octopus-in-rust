use async_trait::async_trait;
use kosong::tooling::{CallableTool2, ToolReturnValue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskOutputParams {
    pub task_id: String,
    #[serde(default)]
    pub block: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
impl CallableTool2 for TaskOutputTool {
    type Params = TaskOutputParams;

    fn name(&self) -> &str {
        "TaskOutput"
    }

    fn description(&self) -> &str {
        "Get the output of a background task."
    }

    async fn call_typed(&self, params: TaskOutputParams) -> ToolReturnValue {
        let (status, output) = match self.bg_manager.get_output(&params.task_id).await {
            Some(result) => result,
            None => return ToolReturnValue::error(format!("Task '{}' not found", params.task_id)),
        };

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
                let result = match self.bg_manager.get_output(&params.task_id).await {
                    Some(r) => r,
                    None => {
                        return ToolReturnValue::error(format!(
                            "Task '{}' disappeared",
                            params.task_id
                        ));
                    }
                };
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
            return ToolReturnValue::ok(format!(
                "Status: {}\n\nOutput:\n{}",
                final_status, current_output
            ));
        }

        ToolReturnValue::ok(format!("Status: {}\n\nOutput:\n{}", status_str, output))
    }
}

#[async_trait]
impl CallableTool2 for TaskStopTool {
    type Params = TaskStopParams;

    fn name(&self) -> &str {
        "TaskStop"
    }

    fn description(&self) -> &str {
        "Stop a background task."
    }

    async fn call_typed(&self, params: TaskStopParams) -> ToolReturnValue {
        if let Err(e) = self.bg_manager.stop(&params.task_id).await {
            return ToolReturnValue::error(format!("Failed to stop task: {}", e));
        }

        ToolReturnValue::ok(format!(
            "Task {} stopped. Reason: {}",
            params.task_id, params.reason
        ))
    }
}
