use std::collections::HashMap;
use std::sync::Arc;

use kosong::tooling::{CallableTool, HandleResult, Tool, ToolResult, Toolset};

/// Validate a tool name against OpenAI function-name rules.
///
/// Names must be 1–64 characters, start with an ASCII letter, and contain only
/// ASCII letters, digits, underscores, and dashes.
pub fn is_valid_tool_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() || name.len() > 64 {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A source of tools that can be loaded into a [`ToolRegistry`].
///
/// Loading is synchronous so that `Brain::new` can remain synchronous. If a
/// source needs async work, it should be performed before constructing the
/// source and the loaded tools cached inside it.
pub trait ToolSource: Send + Sync {
    /// Human-readable name for logging/diagnostics.
    fn name(&self) -> &str;

    /// Load tools from this source.
    ///
    /// Each tool is paired with a source label that identifies where it came
    /// from (e.g. `"host"`, `"faf_units_plugin"`, `"example_http"`). This lets
    /// UIs and status output group tools by their originating plugin.
    fn load_tools(&self) -> Vec<(String, Box<dyn CallableTool>)>;
}

/// Empty tool source used when no external sources are configured.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyToolSource;

impl ToolSource for EmptyToolSource {
    fn name(&self) -> &str {
        "empty"
    }

    fn load_tools(&self) -> Vec<(String, Box<dyn CallableTool>)> {
        Vec::new()
    }
}

/// A simple, in-memory tool registry.
///
/// Implements [`kosong::Toolset`] so it can be passed to [`kosong::step`].
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: Arc<std::sync::RwLock<HashMap<String, Arc<dyn CallableTool>>>>,
    sources: Arc<std::sync::RwLock<HashMap<String, String>>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool with a source label.
    ///
    /// Panics if the tool name is not a valid OpenAI function name. Valid names
    /// start with a letter and contain only letters, digits, underscores, and
    /// dashes, up to 64 characters long. This lets the bot fail fast at startup
    /// instead of getting a 400 from the LLM provider at request time.
    pub fn register(&self, tool: Box<dyn CallableTool>, source: impl Into<String>) {
        let name = tool.name().to_string();
        assert!(
            is_valid_tool_name(&name),
            "tool name {:?} is invalid: must start with a letter and contain only letters, digits, underscores, and dashes (max 64 chars)",
            name
        );
        let mut tools = self.tools.write().unwrap();
        tools.insert(name.clone(), Arc::from(tool));
        let mut sources = self.sources.write().unwrap();
        sources.insert(name, source.into());
    }

    /// Register a type-safe [`kosong::tooling::CallableTool2`] via the adapter.
    pub fn register_typed<T: kosong::tooling::CallableTool2 + 'static>(
        &self,
        tool: T,
        source: impl Into<String>,
    ) {
        self.register(
            Box::new(kosong::tooling::CallableTool2Adapter::new(tool)),
            source,
        );
    }

    /// Look up a tool by name.
    pub fn find(&self, name: &str) -> Option<Arc<dyn CallableTool>> {
        self.tools.read().unwrap().get(name).cloned()
    }

    /// Remove a tool by name, returning it if it existed.
    pub fn remove(&self, name: &str) -> Option<Arc<dyn CallableTool>> {
        self.tools.write().unwrap().remove(name)
    }

    /// Return all registered tool names.
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.read().unwrap().keys().cloned().collect()
    }

    /// Return the source label for a registered tool, if known.
    pub fn tool_source(&self, name: &str) -> Option<String> {
        self.sources.read().unwrap().get(name).cloned()
    }

    /// Return all registered tools with their source labels.
    pub fn tool_sources(&self) -> Vec<(String, String)> {
        let sources = self.sources.read().unwrap();
        sources
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
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
                prompt_fragment: t.prompt_fragment().map(|s| s.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_tool_names() {
        assert!(is_valid_tool_name("qqbot_recent_messages"));
        assert!(is_valid_tool_name("faf_units_search"));
        assert!(is_valid_tool_name("a"));
        assert!(is_valid_tool_name("Tool_1_2"));
    }

    #[test]
    fn test_invalid_tool_names() {
        assert!(!is_valid_tool_name(""));
        assert!(!is_valid_tool_name("qqbot::recent_messages"));
        assert!(!is_valid_tool_name("faf::units_search"));
        assert!(!is_valid_tool_name("1tool"));
        assert!(!is_valid_tool_name("tool name"));
        assert!(!is_valid_tool_name("tool.name"));
    }
}
