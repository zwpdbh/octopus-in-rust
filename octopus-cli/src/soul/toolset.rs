use serde_json::Value;

use crate::tools::Tool;
use crate::wire::{ToolCall, ToolResult, ToolReturnValue};

thread_local! {
    static CURRENT_TOOL_CALL: std::cell::RefCell<Option<ToolCall>> = const { std::cell::RefCell::new(None) };
}

pub fn set_current_tool_call(tc: Option<ToolCall>) {
    CURRENT_TOOL_CALL.with(|c| *c.borrow_mut() = tc);
}

pub fn get_current_tool_call() -> Option<ToolCall> {
    CURRENT_TOOL_CALL.with(|c| c.borrow().clone())
}

pub struct KimiToolset {
    registry: crate::tools::ToolRegistry,
}

impl KimiToolset {
    pub fn new() -> Self {
        Self {
            registry: crate::tools::ToolRegistry::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.registry.register(tool);
    }

    pub fn find(&self, name: &str) -> Option<&dyn Tool> {
        self.registry.get(name)
    }

    pub fn tools(&self) -> Vec<&dyn Tool> {
        self.registry.list()
    }

    pub async fn handle(&self, tool_call: &ToolCall) -> ToolResult {
        set_current_tool_call(Some(tool_call.clone()));
        let result = self.handle_inner(tool_call).await;
        set_current_tool_call(None);
        result
    }

    async fn handle_inner(&self, tool_call: &ToolCall) -> ToolResult {
        let tool = match self.registry.get(&tool_call.function.name) {
            Some(t) => t,
            None => {
                return ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    return_value: ToolReturnValue::error(
                        format!("Tool '{}' not found", tool_call.function.name),
                        "Tool not found".to_string(),
                        None,
                    ),
                };
            }
        };

        let arguments: Value = match serde_json::from_str(&tool_call.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                return ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    return_value: ToolReturnValue::error(
                        format!("JSON parse error: {e}"),
                        "Invalid arguments".to_string(),
                        None,
                    ),
                };
            }
        };

        let tool_call_id = tool_call.id.clone();
        let ret = tool.call(arguments).await;

        let return_value = match ret {
            Ok(output) => ToolReturnValue::ok(
                Some(vec![crate::wire::ContentPart::Text { text: output }]),
                None,
            ),
            Err(output) => ToolReturnValue::error(output.clone(), output, None),
        };

        ToolResult {
            tool_call_id,
            return_value,
        }
    }

    pub fn mcp_status_snapshot(&self) -> Option<crate::wire::MCPStatusSnapshot> {
        None
    }

    pub async fn start_deferred_mcp_tool_loading(&self) -> bool {
        false
    }

    pub async fn wait_for_mcp_tools(&self) {}
}

impl Default for KimiToolset {
    fn default() -> Self {
        Self::new()
    }
}
