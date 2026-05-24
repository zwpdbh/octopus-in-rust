use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session_state::TodoStatus;
use crate::tools::Tool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetTodoListParams {
    pub todos: Vec<TodoItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub title: String,
    pub status: TodoStatus,
}

pub struct SetTodoListTool;

impl SetTodoListTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for SetTodoListTool {
    fn name(&self) -> &str {
        "SetTodoList"
    }

    fn description(&self) -> &str {
        "Set the todo list for the current session."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "SetTodoList",
            "description": "Set the todo list for the current session.",
            "parameters": {
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "status": { "type": "string", "enum": ["pending", "in_progress", "done"] }
                                // Note: JSON schema stays as strings for LLM compatibility;
                                // deserialization maps to TodoStatus enum.
                            },
                            "required": ["title", "status"]
                        }
                    }
                },
                "required": ["todos"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: SetTodoListParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        Ok(format!(
            "Todo list updated with {} items.",
            params.todos.len()
        ))
    }
}
