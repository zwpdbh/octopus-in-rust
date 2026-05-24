use std::collections::{HashMap, HashSet};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::mcp::{McpConfig, McpServerInfo, McpServerStatus, McpToolInfo};
use crate::tools::Tool;
use crate::wire::{ContentPart, ToolCall, ToolOutput, ToolResult, ToolReturnValue};

thread_local! {
    static CURRENT_TOOL_CALL: std::cell::RefCell<Option<ToolCall>> = const { std::cell::RefCell::new(None) };
}

pub fn set_current_tool_call(tc: Option<ToolCall>) {
    CURRENT_TOOL_CALL.with(|c| *c.borrow_mut() = tc);
}

pub fn get_current_tool_call() -> Option<ToolCall> {
    CURRENT_TOOL_CALL.with(|c| c.borrow().clone())
}

const DEDUP_REMINDER_TEXT: &str = "\n\n<system-reminder>\n\
    You are repeating the exact same tool call with identical parameters.\
    Please carefully analyze the previous result. If the task is not yet complete,\
    try a different method or parameters instead of repeating the same call.\
    \n</system-reminder>";

/// Append dedup reminder text to a [`ToolReturnValue`] output.
fn append_dedup_reminder(mut rv: ToolReturnValue) -> ToolReturnValue {
    let reminder = DEDUP_REMINDER_TEXT.to_string();

    match &mut rv.output {
        None => {
            rv.output = Some(ToolOutput::Parts(vec![ContentPart::Text {
                text: reminder,
            }]));
        }
        Some(ToolOutput::Text(text)) => {
            text.push_str(&reminder);
        }
        Some(ToolOutput::Parts(parts)) => {
            if let Some(ContentPart::Text { text }) = parts.last_mut() {
                text.push_str(&reminder);
            } else {
                parts.push(ContentPart::Text { text: reminder });
            }
        }
    }

    rv
}

pub struct KimiToolset {
    registry: crate::tools::ToolRegistry,
    hidden_tools: HashSet<String>,
    hook_engine: Option<crate::hooks::HookEngine>,
    session_id: String,
    cwd: String,
    // Deduplication state (mirrors Python KimiToolset)
    previous_step_calls: Vec<(String, String)>,
    current_step_calls: Vec<(String, String)>,
    current_step_results: HashMap<(String, String), ToolResult>,
    dedup_triggered: bool,
    step_no: usize,
    turn_id: String,
    // MCP state
    mcp_servers: HashMap<String, McpServerInfo>,
    deferred_mcp_load: Option<(Vec<McpConfig>, McpLoadContext)>,
    mcp_loading_task: Option<tokio::task::JoinHandle<()>>,
}

/// Context needed for deferred MCP loading.
#[derive(Clone)]
pub struct McpLoadContext {
    pub tool_call_timeout_ms: u64,
}

impl KimiToolset {
    pub fn new() -> Self {
        Self {
            registry: crate::tools::ToolRegistry::new(),
            hidden_tools: HashSet::new(),
            hook_engine: None,
            session_id: String::new(),
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            previous_step_calls: Vec::new(),
            current_step_calls: Vec::new(),
            current_step_results: HashMap::new(),
            dedup_triggered: false,
            step_no: 0,
            turn_id: String::new(),
            mcp_servers: HashMap::new(),
            deferred_mcp_load: None,
            mcp_loading_task: None,
        }
    }

    pub fn set_hook_engine(&mut self, engine: crate::hooks::HookEngine) {
        self.hook_engine = Some(engine);
    }

    pub fn set_session_id(&mut self, id: String) {
        self.session_id = id;
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.registry.register(tool);
    }

    pub fn find(&self, name: &str) -> Option<&dyn Tool> {
        self.registry.get(name)
    }

    /// Hide a tool from the LLM tool list. Returns `true` if the tool exists.
    pub fn hide(&mut self, tool_name: &str) -> bool {
        if self.registry.get(tool_name).is_some() {
            self.hidden_tools.insert(tool_name.to_string());
            true
        } else {
            false
        }
    }

    /// Restore a hidden tool to the LLM tool list.
    pub fn unhide(&mut self, tool_name: &str) {
        self.hidden_tools.remove(tool_name);
    }

    pub fn tools(&self) -> Vec<&dyn Tool> {
        self.registry
            .list()
            .into_iter()
            .filter(|t| !self.hidden_tools.contains(t.name()))
            .collect()
    }

    /// Called before each step to set up deduplication state.
    pub fn begin_step(
        &mut self,
        previous_calls: Vec<(String, String)>,
        step_no: usize,
        turn_id: String,
    ) {
        self.previous_step_calls = previous_calls;
        self.current_step_calls = Vec::new();
        self.current_step_results = HashMap::new();
        self.dedup_triggered = false;
        self.step_no = step_no;
        self.turn_id = turn_id;
    }

    /// Called after each step to capture the calls made in this step.
    pub fn end_step(&self) -> Vec<(String, String)> {
        self.current_step_calls.clone()
    }

    /// Whether a cross-step duplicate was blocked in the current step.
    pub fn dedup_triggered(&self) -> bool {
        self.dedup_triggered
    }

    pub async fn handle(&mut self, tool_call: &ToolCall) -> ToolResult {
        set_current_tool_call(Some(tool_call.clone()));
        let result = self.handle_inner(tool_call).await;
        set_current_tool_call(None);
        result
    }

    async fn handle_inner(&mut self, tool_call: &ToolCall) -> ToolResult {
        let call_key = (
            tool_call.function.name.clone(),
            tool_call.function.arguments.clone(),
        );

        // --- Same-step dedup: wait for the original result and copy it ---
        if let Some(original) = self.current_step_results.get(&call_key) {
            tracing::warn!(
                "Same-step dedup detected for tool '{}' at step {}",
                call_key.0,
                self.step_no
            );
            let args_hash = format!("{:x}", Sha256::digest(call_key.1.as_bytes()));
            crate::track!(
                "tool_call_dedup_detected",
                session_id = self.session_id,
                turn_id = self.turn_id,
                step_no = self.step_no,
                tool_name = call_key.0,
                dup_type = "same_step",
                args_hash = &args_hash[..8.min(args_hash.len())],
            );
            return ToolResult {
                tool_call_id: tool_call.id.clone(),
                return_value: original.return_value.clone(),
            };
        }

        let is_cross_step_dup = self.previous_step_calls.contains(&call_key);
        if is_cross_step_dup {
            tracing::warn!(
                "Cross-step dedup detected for tool '{}' at step {}",
                call_key.0,
                self.step_no
            );
            let args_hash = format!("{:x}", Sha256::digest(call_key.1.as_bytes()));
            crate::track!(
                "tool_call_dedup_detected",
                session_id = self.session_id,
                turn_id = self.turn_id,
                step_no = self.step_no,
                tool_name = call_key.0,
                dup_type = "cross_step",
                args_hash = &args_hash[..8.min(args_hash.len())],
            );
            self.dedup_triggered = true;
        }

        let tool = match self.registry.get(&tool_call.function.name) {
            Some(t) => t,
            None => {
                let result = ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    return_value: ToolReturnValue::error(
                        format!("Tool '{}' not found", tool_call.function.name),
                        "Tool not found".to_string(),
                        None,
                    ),
                };
                self.current_step_results
                    .insert(call_key.clone(), result.clone());
                self.current_step_calls.push(call_key);
                return result;
            }
        };

        let arguments: Value = match serde_json::from_str(&tool_call.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                let result = ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    return_value: ToolReturnValue::error(
                        format!("JSON parse error: {e}"),
                        "Invalid arguments".to_string(),
                        None,
                    ),
                };
                self.current_step_results
                    .insert(call_key.clone(), result.clone());
                self.current_step_calls.push(call_key);
                return result;
            }
        };

        // --- PreToolUse hook ---
        let tool_input_map = match arguments.as_object() {
            Some(obj) => obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            None => std::collections::HashMap::new(),
        };
        if let Some(ref engine) = self.hook_engine {
            if engine.has_hooks_for("PreToolUse") {
                let input_data = crate::hooks::events::pre_tool_use(
                    &self.session_id,
                    &self.cwd,
                    &tool_call.function.name,
                    &tool_input_map,
                    &tool_call.id,
                );
                let results = engine
                    .trigger("PreToolUse", &tool_call.function.name, input_data)
                    .await;
                for r in &results {
                    if let crate::hooks::runner::HookAction::Block(ref reason) = r.action {
                        let result = ToolResult {
                            tool_call_id: tool_call.id.clone(),
                            return_value: ToolReturnValue::error(
                                reason.clone(),
                                "Hook blocked".to_string(),
                                None,
                            ),
                        };
                        self.current_step_results
                            .insert(call_key.clone(), result.clone());
                        self.current_step_calls.push(call_key);
                        return result;
                    }
                }
            }
        }

        let tool_call_id = tool_call.id.clone();
        let t0 = std::time::Instant::now();
        let ret = tool.call(arguments).await;
        let elapsed = t0.elapsed();
        let duration_ms = elapsed.as_millis() as u64;

        let return_value = match &ret {
            Ok(output) => {
                crate::track!(
                    "tool_call",
                    tool_name = tool_call.function.name,
                    outcome = "success",
                    duration_ms = duration_ms,
                );
                ToolReturnValue::ok(
                    Some(vec![ContentPart::Text {
                        text: output.clone(),
                    }]),
                    None,
                )
            }
            Err(output) => {
                crate::track!(
                    "tool_call",
                    tool_name = tool_call.function.name,
                    outcome = "error",
                    duration_ms = duration_ms,
                );
                ToolReturnValue::error(output.clone(), output.clone(), None)
            }
        };

        let mut result = ToolResult {
            tool_call_id,
            return_value,
        };

        if is_cross_step_dup {
            result.return_value = append_dedup_reminder(result.return_value.clone());
        }

        // --- PostToolUse / PostToolUseFailure hooks (fire-and-forget) ---
        if let Some(ref engine) = self.hook_engine {
            match ret {
                Ok(ref output) => {
                    if engine.has_hooks_for("PostToolUse") {
                        let input_data = crate::hooks::events::post_tool_use(
                            &self.session_id,
                            &self.cwd,
                            &tool_call.function.name,
                            &tool_input_map,
                            &output[..output.len().min(2000)],
                            &tool_call.id,
                        );
                        let _ = engine.fire_and_forget_trigger(
                            "PostToolUse",
                            &tool_call.function.name,
                            input_data,
                        );
                    }
                }
                Err(ref error) => {
                    if engine.has_hooks_for("PostToolUseFailure") {
                        let input_data = crate::hooks::events::post_tool_use_failure(
                            &self.session_id,
                            &self.cwd,
                            &tool_call.function.name,
                            &tool_input_map,
                            error,
                            &tool_call.id,
                        );
                        let _ = engine.fire_and_forget_trigger(
                            "PostToolUseFailure",
                            &tool_call.function.name,
                            input_data,
                        );
                    }
                }
            }
        }

        self.current_step_results
            .insert(call_key.clone(), result.clone());
        self.current_step_calls.push(call_key);

        result
    }

    // ========================================================================
    // External tools (Wire)
    // ========================================================================

    /// Register an external tool delivered via the wire protocol.
    /// Returns `(ok, reason)` where `reason` is None on success.
    pub fn register_external_tool(
        &mut self,
        name: &str,
        description: &str,
        parameters: serde_json::Map<String, serde_json::Value>,
    ) -> (bool, Option<String>) {
        if self.registry.get(name).is_some() {
            return (
                false,
                Some("tool name conflicts with existing tool".to_string()),
            );
        }
        let tool = WireExternalTool {
            name: name.to_string(),
            description: description.to_string(),
            parameters: Value::Object(parameters),
        };
        self.registry.register(Box::new(tool));
        (true, None)
    }

    // ========================================================================
    // MCP
    // ========================================================================

    pub fn mcp_servers(&self) -> &HashMap<String, McpServerInfo> {
        &self.mcp_servers
    }

    pub fn mcp_status_snapshot(&self) -> Option<crate::wire::MCPStatusSnapshot> {
        if self.mcp_servers.is_empty() {
            return None;
        }

        let servers: Vec<crate::wire::MCPServerSnapshot> = self
            .mcp_servers
            .iter()
            .map(|(name, info)| crate::wire::MCPServerSnapshot {
                name: name.clone(),
                status: info.status.as_str().to_string(),
                tools: info.tools.iter().map(|t| t.name.clone()).collect(),
            })
            .collect();

        Some(crate::wire::MCPStatusSnapshot {
            loading: self.has_pending_mcp_tools(),
            connected: self
                .mcp_servers
                .values()
                .filter(|s| s.status == McpServerStatus::Connected)
                .count(),
            total: self.mcp_servers.len(),
            tools: self.mcp_servers.values().map(|s| s.tools.len()).sum(),
            servers,
        })
    }

    pub fn defer_mcp_tool_loading(&mut self, configs: Vec<McpConfig>, context: McpLoadContext) {
        self.deferred_mcp_load = Some((configs, context));
    }

    pub fn has_deferred_mcp_tools(&self) -> bool {
        self.deferred_mcp_load.is_some()
    }

    pub async fn start_deferred_mcp_tool_loading(&mut self) -> bool {
        if self.deferred_mcp_load.is_none() {
            return false;
        }
        if self.mcp_loading_task.is_some() || !self.mcp_servers.is_empty() {
            self.deferred_mcp_load = None;
            return false;
        }

        let (configs, _context) = self.deferred_mcp_load.take().unwrap();
        self.load_mcp_tools(configs, true).await;
        true
    }

    pub fn has_pending_mcp_tools(&self) -> bool {
        self.mcp_loading_task
            .as_ref()
            .map(|t| !t.is_finished())
            .unwrap_or(false)
    }

    pub async fn wait_for_mcp_tools(&mut self) {
        let task = self.mcp_loading_task.take();
        if let Some(t) = task {
            let _ = t.await;
        }
    }

    async fn load_mcp_tools(&mut self, configs: Vec<McpConfig>, in_background: bool) {
        // Set up pending server entries from config.
        for config in &configs {
            for (server_name, _server_config) in &config.servers {
                if !self.mcp_servers.contains_key(server_name) {
                    self.mcp_servers.insert(
                        server_name.clone(),
                        McpServerInfo::new(McpServerStatus::Pending),
                    );
                }
            }
        }

        let connect = async move {
            tracing::info!("MCP tool loading started");
            for config in configs {
                for (server_name, server_config) in config.servers {
                    tracing::info!(
                        "Connecting to MCP server '{}' (transport: {})",
                        server_name,
                        server_config.transport
                    );
                    // TODO: implement real MCP client connection using a Rust MCP library.
                    // For now, mark all servers as failed with a clear message.
                    tracing::warn!(
                        "MCP client not yet implemented in Rust; server '{}' marked as failed",
                        server_name
                    );
                }
            }
            tracing::info!("MCP tool loading finished");
        };

        if in_background {
            self.mcp_loading_task = Some(tokio::spawn(connect));
        } else {
            connect.await;
        }
    }

    pub async fn cleanup_mcp(&mut self) {
        self.deferred_mcp_load = None;
        if let Some(task) = self.mcp_loading_task.take() {
            task.abort();
            let _ = task.await;
        }
        self.mcp_servers.clear();
    }
}

impl Default for KimiToolset {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// WireExternalTool
// ============================================================================

/// A tool that delegates execution to an external client via the wire protocol.
pub struct WireExternalTool {
    name: String,
    description: String,
    parameters: Value,
}

#[async_trait::async_trait]
impl Tool for WireExternalTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Value {
        self.parameters.clone()
    }

    async fn call(&self, _args: Value) -> Result<String, String> {
        // The actual call is handled by the wire server — this should not be
        // invoked directly. If it is, return an error explaining the issue.
        Err(format!(
            "External tool '{}' must be called through the wire protocol, not directly.",
            self.name
        ))
    }
}

// ============================================================================
// MCPTool (stub)
// ============================================================================

/// A tool backed by an MCP server. Stub — real implementation needs a Rust MCP client.
pub struct MCPTool {
    server_name: String,
    tool_info: McpToolInfo,
}

impl MCPTool {
    pub fn new(server_name: String, tool_info: McpToolInfo) -> Self {
        Self {
            server_name,
            tool_info,
        }
    }
}

#[async_trait::async_trait]
impl Tool for MCPTool {
    fn name(&self) -> &str {
        &self.tool_info.name
    }

    fn description(&self) -> &str {
        &self.tool_info.description
    }

    fn schema(&self) -> Value {
        self.tool_info.schema.clone()
    }

    async fn call(&self, _args: Value) -> Result<String, String> {
        Err(format!(
            "MCP tool '{}' from server '{}' is not yet callable (MCP client not implemented).",
            self.tool_info.name, self.server_name
        ))
    }
}
