use std::collections::HashMap;
use std::sync::Arc;

use kosong::tooling::{CallableTool, HandleResult, Tool, ToolResult, Toolset};

/// A source of tools that can be loaded into a [`ToolRegistry`].
///
/// Loading is synchronous so that `Brain::new` can remain synchronous. If a
/// source needs async work, it should be performed before constructing the
/// source and the loaded tools cached inside it.
pub trait ToolSource: Send + Sync {
    /// Human-readable name for logging/diagnostics.
    fn name(&self) -> &str;

    /// Load tools from this source.
    fn load_tools(&self) -> Vec<Box<dyn CallableTool>>;
}

/// Empty tool source used when no external sources are configured.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyToolSource;

impl ToolSource for EmptyToolSource {
    fn name(&self) -> &str {
        "empty"
    }

    fn load_tools(&self) -> Vec<Box<dyn CallableTool>> {
        Vec::new()
    }
}

/// A simple, in-memory tool registry.
///
/// Implements [`kosong::Toolset`] so it can be passed to [`kosong::step`].
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: Arc<std::sync::RwLock<HashMap<String, Arc<dyn CallableTool>>>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool.
    pub fn register(&self, tool: Box<dyn CallableTool>) {
        let name = tool.name().to_string();
        let mut tools = self.tools.write().unwrap();
        tools.insert(name, Arc::from(tool));
    }

    /// Register a type-safe [`kosong::tooling::CallableTool2`] via the adapter.
    pub fn register_typed<T: kosong::tooling::CallableTool2 + 'static>(&self, tool: T) {
        self.register(Box::new(kosong::tooling::CallableTool2Adapter::new(tool)));
    }

    /// Look up a tool by name.
    pub fn find(&self, name: &str) -> Option<Arc<dyn CallableTool>> {
        self.tools.read().unwrap().get(name).cloned()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.read().unwrap().len()
    }

    /// Whether the registry has no tools.
    pub fn is_empty(&self) -> bool {
        self.tools.read().unwrap().is_empty()
    }
}

impl Toolset for ToolRegistry {
    fn tools(&self) -> Vec<Tool> {
        let tools = self.tools.read().unwrap();
        tools
            .values()
            .map(|t| Tool {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
            })
            .collect()
    }

    fn handle(&self, tool_call: &kosong::ToolCall) -> HandleResult {
        let name = &tool_call.function.name;
        let Some(tool) = self.find(name) else {
            return HandleResult::Ready(ToolResult {
                tool_call_id: tool_call.id.clone(),
                return_value: kosong::tooling::ToolReturnValue::error(format!(
                    "Tool '{}' not found",
                    name
                )),
            });
        };

        let args = match tool_call.function.arguments.as_ref() {
            Some(s) => match serde_json::from_str(s) {
                Ok(v) => v,
                Err(e) => {
                    return HandleResult::Ready(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        return_value: kosong::tooling::ToolReturnValue::error(format!(
                            "JSON parse error: {e}"
                        )),
                    });
                }
            },
            None => serde_json::Value::Null,
        };

        let tc_id = tool_call.id.clone();
        let handle = tokio::spawn(async move {
            let return_value = tool.call_raw(args).await;
            ToolResult {
                tool_call_id: tc_id,
                return_value,
            }
        });
        HandleResult::Pending(handle)
    }
}
