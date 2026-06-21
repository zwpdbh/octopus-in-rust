use crate::message::ToolCall;
use crate::tooling::{CallableTool, HandleResult, Tool, ToolResult, ToolReturnValue, Toolset};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// A simple toolset that can handle tool calls concurrently.
pub struct SimpleToolset {
    tools: HashMap<String, Arc<dyn CallableTool>>,
}

impl SimpleToolset {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn add(&mut self, tool: Arc<dyn CallableTool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn remove(&mut self, tool_name: &str) {
        if self.tools.remove(tool_name).is_none() {
            panic!("Tool `{tool_name}` not found in the toolset.");
        }
    }
}

impl Default for SimpleToolset {
    fn default() -> Self {
        Self::new()
    }
}

impl Toolset for SimpleToolset {
    fn tools(&self) -> Vec<Tool> {
        self.tools
            .values()
            .map(|t| Tool {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
                prompt_fragment: t.prompt_fragment().map(|s| s.to_string()),
            })
            .collect()
    }

    fn handle(&self, tool_call: &ToolCall) -> HandleResult {
        let tool = match self.tools.get(&tool_call.function.name) {
            Some(t) => Arc::clone(t),
            None => {
                return HandleResult::Ready(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    return_value: ToolReturnValue::error(format!(
                        "Tool '{}' not found",
                        tool_call.function.name
                    )),
                });
            }
        };

        let args: Value =
            match serde_json::from_str(tool_call.function.arguments.as_deref().unwrap_or("{}")) {
                Ok(v) => v,
                Err(e) => {
                    return HandleResult::Ready(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        return_value: ToolReturnValue::error(format!("JSON parse error: {e}")),
                    });
                }
            };

        let tool_call_id = tool_call.id.clone();
        let handle = tokio::spawn(async move {
            let return_value = tool.call_raw(args).await;
            ToolResult {
                tool_call_id,
                return_value,
            }
        });

        HandleResult::Pending(handle)
    }
}
