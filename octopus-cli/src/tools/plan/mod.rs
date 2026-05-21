use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnterPlanModeParams {
    #[serde(default)]
    pub reason: String,
}

pub struct EnterPlanModeTool;
pub struct ExitPlanModeTool;

impl EnterPlanModeTool {
    pub fn new() -> Self {
        Self
    }
}

impl ExitPlanModeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }

    fn description(&self) -> &str {
        "Enter plan mode to create a read-only plan before making changes."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "EnterPlanMode",
            "description": "Enter plan mode to create a read-only plan before making changes.",
            "parameters": {
                "type": "object",
                "properties": {
                    "reason": { "type": "string", "description": "Reason for entering plan mode" }
                }
            }
        })
    }

    async fn call(&self, _arguments: Value) -> Result<String, String> {
        Ok("Plan mode activated. All file changes must go through the plan file.".to_string())
    }
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn description(&self) -> &str {
        "Exit plan mode and begin executing the plan."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "ExitPlanMode",
            "description": "Exit plan mode and begin executing the plan.",
            "parameters": {
                "type": "object",
                "properties": {}
            }
        })
    }

    async fn call(&self, _arguments: Value) -> Result<String, String> {
        Ok("Plan mode deactivated. You can now make direct changes.".to_string())
    }
}
