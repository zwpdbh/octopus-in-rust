# Tour 3: The Tool Shed — Where Action Happens

> *"The Control Room decides. The Tool Shed executes. Every file read, every shell command, every web search starts here."*

Welcome to the **Tool Shed** — the west wing of the second floor. If the Control Room (`KimiSoul`) is the brain, the Tool Shed is the **hands**. This is where the agent interacts with the outside world: reading files, running shell commands, searching the web, and more.

In this tour, we'll examine the **Tool trait**, the **tool registry**, the **execution pipeline**, and a few star tools.

---

## 🔧 The Universal Wrench: `kosong::CallableTool`

File: `kosong/src/tooling.rs` + `octopus-cli/src/tools/*/mod.rs`

Every tool in the shed implements the kosong trait directly — there is no parallel octopus trait layer:

```rust
// File: kosong/src/tooling.rs
#[async_trait]
pub trait CallableTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;        // JSON Schema
    async fn call_raw(&self, arguments: Value) -> ToolReturnValue;
}
```

For static tools, we use the type-safe `CallableTool2` trait with an associated `Params` type:

```rust
// File: octopus-cli/src/tools/shell/mod.rs
#[async_trait]
impl CallableTool2 for ShellTool {
    type Params = ShellParams;  // Derives JsonSchema automatically

    async fn call_typed(&self, params: ShellParams) -> ToolReturnValue {
        // params is already deserialized and validated
        ...
    }
}
```

🐍 **Python's way:** Tools are classes inheriting from a base `Tool` class. Schema is built via Pydantic models or `@property` methods.

🦀 **Rust's way:** A trait from the kosong crate with `async_trait`. The `Send + Sync` bounds mean tools can be shared across threads — critical for concurrent execution. `CallableTool2Adapter` bridges `CallableTool2` → `CallableTool` by deriving the JSON Schema via `schemars::schema_for!()`.

✨ **Where Rust shines:** **Schema is code.** `CallableTool2` uses `schemars::schema_for!(Params)` to generate the JSON Schema at compile time. For dynamic tools, `CallableTool::parameters()` returns a `Value` built at runtime — e.g. `McpTool` fetches its schema from the MCP server at connection time:

```rust
// File: octopus-cli/src/mcp/tool.rs
#[async_trait]
impl kosong::tooling::CallableTool for McpTool {
    fn parameters(&self) -> Value {
        self.schema.get("parameters").cloned().unwrap_or(...)
    }
}
```

---

## 🗃️ The Tool Registry: `KimiToolset`

File: `octopus-cli/src/soul/toolset.rs` (~857 lines)

The `KimiToolset` is the **shed's inventory system**. It knows every tool and handles execution:

```rust
// File: octopus-cli/src/soul/toolset.rs
pub struct KimiToolset {
    tools: HashMap<String, Box<dyn kosong::tooling::CallableTool>>,
    /// Tool names that are registered but hidden from the LLM tool list.
    /// Used for subagent `ToolPolicy::AllowList` — register all tools once,
    /// then hide the ones the agent spec disallows.
    hidden_tools: HashSet<String>,
    hook_engine: Option<HookEngine>,
    session_id: String,
    cwd: String,
    step_state: Mutex<StepState>,
    mcp_servers: HashMap<String, McpServerInfo>,
    mcp_state: Mutex<McpState>,
    approval: Mutex<Option<Approval>>,
}
```

### Registration

In Python, tools were hardcoded in `KimiSoul.__init__`. In the Rust rewrite, tools are **loaded from the agent spec**:

```rust
// In soul/agent.rs::load_agent()
let spec = load_agent_spec(agent_file)?;
let mut toolset = KimiToolset::new();

for tool in spec.tools {
    if let Some(tool) = build_tool(tool, &runtime) {
        toolset.register(tool);
    }
}
```

`load_agent_spec()` parses raw tool name strings into the `BuiltinTool` enum — typos and unknown tools are caught at load time with a warning, not silently passed through. `build_tool()` then uses an exhaustive `match` on the enum to map each variant to its Rust constructor. The agent spec drives the toolbox — no hardcoded list.

🐍 **Python's way:** Tools are registered via `toolset.register_tool(MyTool())` in a list comprehension or loop. Agent spec parsing + dynamic import via `importlib`.

🦀 **Rust's way:** Static name-to-constructor mapping in `build_tool()` via exhaustive `match` on `BuiltinTool`. No dynamic imports — the compiler ensures every mapped tool exists, and adding a new builtin tool becomes a compile error until you update the match. `Box<dyn kosong::CallableTool>` is a **fat pointer** (data + vtable), but lookup is O(1) via `HashMap<String, Box<dyn CallableTool>>`.

✨ **Where Rust shines:** **Agent-driven toolset.** Change `agents/default/agent.yaml` and the soul boots with a different toolbox — no recompilation needed for config changes, but the compiler still validates every tool constructor at build time.

---

## ⚡ The Execution Pipeline

When the LLM requests a tool, this pipeline runs:

```rust
// File: octopus-cli/src/soul/toolset.rs
async fn handle_inner(&self, tool_call: &kosong::ToolCall) -> kosong::ToolResult {
    // 1. Deduplication check (same-step + cross-step)
    if let Some(cached) = state.current_step_results.get(&call_key) {
        return cached.clone();
    }

    // 2. Find the tool
    let tool = self.tools.get(&tool_call.function.name).unwrap();

    // 3. Run PreToolUse hook
    if let HookAction::Block { reason } = self.run_pre_hook(...).await {
        return kosong::ToolResult {
            tool_call_id: tool_call.id.clone(),
            return_value: kosong::ToolReturnValue::error(format!("Blocked by hook: {}", reason)),
        };
    }

    // 4. Request approval (if not yolo/afk)
    if let Some(approval) = &self.approval {
        match approval.request(...).await {
            ApprovalResponse::Approve => {}
            ApprovalResponse::Reject { feedback } => {
                return kosong::ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    return_value: kosong::ToolReturnValue::error(format!("User rejected: {}", feedback)),
                };
            }
        }
    }

    // 5. Execute!
    let start = Instant::now();
    let return_value = tool.call_raw(arguments).await;
    let elapsed = start.elapsed();

    // 6. Run PostToolUse / PostToolUseFailure hook
    self.run_post_hook(...).await;

    // 7. Track telemetry
    track!("tool_call", tool_name = name, duration_ms = ...);

    kosong::ToolResult { tool_call_id, return_value }
}
```

This pipeline has **four guardrails** before the tool actually runs:
1. **Hook check** — can the tool run in this context?
2. **Approval check** — does the user want to allow this?
3. **Execution** — the actual work
4. **Post-hook + telemetry** — cleanup and logging

🐍 **Python's way:** Similar pipeline, but hooks and approval are mixed into the tool class hierarchy.

🦀 **Rust's way:** The pipeline is **orthogonal** to tools. Every tool gets the same guards automatically. The `CallableTool` trait only defines `call_raw()` — everything else is `KimiToolset` middleware.

---

## 🔌 The Kosong Bridge: `KimiToolsetHandle`

File: `octopus-cli/src/soul/toolset.rs` (near the bottom)

The Tool Shed doesn't just serve the local `KimiSoul`. It also serves the **`kosong` LLM abstraction layer**. `kosong::step()` expects a `&dyn kosong::Toolset`, and `KimiToolset` is the natural owner of the tools.

Because Rust's orphan rules prevent implementing `kosong::Toolset` for `Arc<KimiToolset>` directly (both are foreign to each other's crates), we use a thin **newtype wrapper**:

```rust
pub struct KimiToolsetHandle(pub Arc<KimiToolset>);

impl kosong::Toolset for KimiToolsetHandle {
    fn tools(&self) -> Vec<kosong::Tool> {
        self.0.tools().into_iter().map(|t| kosong::Tool {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters(),
        }).collect()
    }

    fn handle(&self, tool_call: &kosong::ToolCall) -> kosong::HandleResult {
        let inner = Arc::clone(&self.0);
        let tc = tool_call.clone();
        let handle = tokio::spawn(async move { inner.handle_inner(&tc).await });
        kosong::HandleResult::Pending(handle)
    }
}
```

### Why the callback moved to `step_with_callbacks`

The `on_tool_result` callback sends `WireEvent::ToolResult` to the UI as soon as each tool finishes. `kosong::step_with_callbacks` accepts this callback as a parameter, so it lives outside the toolset entirely:

```rust
// Control Room (kimisoul.rs)
let step_result = kosong::step_with_callbacks(
    provider.as_ref(),
    &self.agent.system_prompt,
    &KimiToolsetHandle(self.toolset.clone()),
    &kosong_history,
    Some(&mut on_message_part),
    Some(Arc::new(|result: &kosong::ToolResult| {
        // Convert kosong result → wire result and send to UI
        wire_send(WireEvent::ToolResult(wire_result));
    })),
).await;
```

🐍 **Python's way:** Python `kosong.step` accepts an `on_tool_result` callback directly as a parameter. No adapter needed because Python's dynamic typing handles the conversion implicitly.

🦀 **Rust's way:** The `KimiToolsetHandle` newtype satisfies orphan rules while keeping the bridge minimal. The callback is passed directly to `kosong::step_with_callbacks`, matching Python's API shape. `KimiToolset` implements kosong types natively — no wire/kosong conversion inside the toolset itself.

✨ **Where Rust shines:** **One trait layer, not two.** Python `kimi-cli` uses `kosong.tooling.CallableTool2` directly with no parallel trait. After this refactor, Rust does the same. The old octopus `Tool` + `TypedTool` + `TypedToolAdapter` + `KosongToolsetAdapter` stack collapsed into a single kosong layer.

---

## 🌟 Star Tool: `ShellTool`

File: `octopus-cli/src/tools/shell/mod.rs` (~169 lines)

The `ShellTool` lets the agent run shell commands. It's one of the most powerful — and dangerous — tools.

```rust
// File: octopus-cli/src/tools/shell/mod.rs
pub struct ShellTool {
    bg_manager: BackgroundTaskManager,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ShellParams {
    pub command: String,
    #[serde(default)]
    pub run_in_background: bool,
    #[serde(default)]
    pub timeout: u64,
}

#[async_trait]
impl CallableTool2 for ShellTool {
    type Params = ShellParams;

    async fn call_typed(&self, params: ShellParams) -> ToolReturnValue {
        if params.run_in_background {
            let id = self.bg_manager.spawn(params.command, ...).await;
            ToolReturnValue::ok(format!("Task started: {}", id))
        } else {
            let output = run_command(&params.command).await;
            ToolReturnValue::ok(output)
        }
    }
}
```

Notice the **`run_in_background` flag**. This one parameter branches into two entirely different execution paths:
- **Foreground:** Run now, block until done, return output
- **Background:** Spawn a `tokio::process::Child`, return immediately with a task ID

🐍 **Python's way:** Background tasks are managed by a separate `BackgroundTaskManager` process with SQLite persistence.

🦀 **Rust's way:** The `BackgroundTaskManager` is a struct holding `Arc<Mutex<HashMap<...>>>`. Tasks are spawned as `tokio::spawn`ed futures. No separate process, no SQLite — just Tokio tasks.

✨ **Where Rust shines:** **Tasks are lightweight.** A `tokio::process::Child` is ~1KB of memory. In Python, each background task might involve a `multiprocessing.Process` (~50MB) or a thread (~8MB). Rust can spawn thousands of background tasks without breaking a sweat.

---

## 🌟 Star Tool: `ReadFileTool`

File: `octopus-cli/src/tools/file/mod.rs` (~559 lines)

The `ReadFileTool` is the agent's eyes. It reads files with optional line offsets:

```rust
// File: octopus-cli/src/tools/file/mod.rs
let path = arguments["path"].as_str().unwrap();
let offset = arguments["offset"].as_u64().unwrap_or(0) as usize;
let limit = arguments["limit"].as_u64().unwrap_or(0) as usize;

let content = tokio::fs::read_to_string(path).await?;
let lines: Vec<&str> = content.lines().collect();
let selected = if limit > 0 {
    &lines[offset..(offset + limit).min(lines.len())]
} else {
    &lines[offset..]
};
```

Simple, but critical: the agent uses this to **read code, configs, and logs**.

---

## 🌟 Star Tool: `AgentTool` (The Recursive Wrench)

File: `octopus-cli/src/tools/agent/mod.rs` (~270 lines)

The `AgentTool` is the **most mind-bending tool** in the shed. It creates a **new `KimiSoul`** — a recursive agent call! But unlike a naive recursion, it now goes through a full **type registry and policy enforcement** pipeline.

### The Subagent Spawner (Today)

```rust
// File: octopus-cli/src/tools/agent/mod.rs
pub struct AgentTool {
    parent_runtime: AppRuntime,  // Shared runtime with LaborMarket, skills, etc.
}

#[async_trait]
impl CallableTool2 for AgentTool {
    type Params = AgentParams;

    async fn call_typed(&self, params: AgentParams) -> ToolReturnValue {
        let parent_runtime = self.parent_runtime.clone();

        // Background or foreground?
        if params.run_in_background {
            tokio::spawn(async move {
                run_subagent_with_market(parent_runtime, &params).await
            });
            ToolReturnValue::ok("Launched in background".to_string())
        } else {
            match run_subagent_with_market(parent_runtime, &params).await {
                Ok(output) => ToolReturnValue::ok(output),
                Err(e) => ToolReturnValue::error(e),
            }
        }
    }
}
```

When the main agent calls `AgentTool`, the child agent inherits:
- **`LaborMarket`** — the same subagent type registry (so child can spawn its own subagents)
- **`skills`** — inherited knowledge directories
- **`notifications`** — shared mailroom for cross-agent messaging
- **`background_tasks`** — shared workshop foreman
- **`denwa_renji`** — shared D-Mail/phone system
- **`config`** — same building blueprints

But it gets:
- **Fresh session** — isolated context, no pollution of parent's conversation
- **Model override** — if the agent spec or user params specify a different model
- **ToolPolicy enforcement** — only allowed tools visible to the child

🐍 **Python's way:** Subagents are managed by a `SubagentStore` with registry, builder, and output formatting.

🦀 **Rust's way:** A type-driven pipeline. `LaborMarket` registers subagent types from YAML specs. `AgentTool` looks up types, loads their specs, enforces `ToolPolicy`, and resolves model overrides — all before the child soul takes its first breath.

✨ **Where Rust shines:** **Policy enforcement is compile-time verified.** The `ToolPolicy` enum (`AllowList | Inherit`) is matched exhaustively. Adding a new policy variant is a compile error everywhere it's not handled — in Python, a missing branch would silently fall through to "allow all."

---

## 🔌 The MCP Adapter: External Tools

File: `octopus-cli/src/mcp/client.rs` (~330 lines)

The Tool Shed has a special door for **external tools** — MCP (Model Context Protocol) servers. These are separate processes that expose tools via JSON-RPC over stdio.

```rust
// File: octopus-cli/src/mcp/client.rs
let client = McpClient::connect_stdio("npx", vec!["-y", "@modelcontextprotocol/server-filesystem", "/home"], None).await?;
let tools = client.list_tools().await?;
```

The MCP client:
1. Spawns the server process
2. Performs an `initialize` handshake
3. Calls `tools/list` to discover available tools
4. Wraps each remote tool as a local `McpTool` implementing `kosong::CallableTool`

🐍 **Python's way:** `mcp` Python SDK handles transport and schema conversion.

🦀 **Rust's way:** Hand-rolled JSON-RPC over stdio. ~330 lines total — smaller than many SDK wrappers. Why hand-rolled? Because `rust-mcp-sdk` had tokio compatibility concerns, and our custom client fits our `Tool` trait exactly.

---

## 🧩 The Plugin Dock: WASM Plugins

File: `octopus-cli/src/plugin/discovery.rs` (~280 lines) + `plugin/manifest.rs`

The Tool Shed has a **Plugin Dock** for WebAssembly (WASM) plugins. These are sandboxed, portable extensions written in any language that compiles to WASM.

### Security-First by Design

Every plugin ships with a JSON manifest that defines its permissions:

```json
{
  "name": "HttpRequest",
  "description": "Make HTTP requests",
  "schema": { ... },
  "allowed_hosts": ["api.github.com", "httpbin.org"],
  "allowed_paths": {},
  "timeout_ms": 30000,
  "max_memory_pages": 64
}
```

The manifest uses **deny-by-default** security:
- `allowed_hosts: []` or omitted → **no network access**
- `allowed_paths: {}` → **no filesystem access**
- `timeout_ms` → hard limit on execution time
- `max_memory_pages` → memory ceiling

```rust
// File: octopus-cli/src/plugin/discovery.rs
pub fn build_extism_manifest(manifest: &PluginManifest) -> extism::Manifest {
    let mut m = extism::Manifest::new([wasm_path])
        .with_timeout(Duration::from_millis(timeout));
    
    match allowed_hosts {
        Some(hosts) if !hosts.is_empty() => {
            m = m.with_allowed_hosts(hosts);
        }
        _ => {
            m = m.disallow_all_hosts();  // Deny by default!
        }
    }
    m
}
```

🐍 **Python's way:** Python plugins run with full process privileges. Sandboxing requires separate processes, seccomp, or containers.

🦀 **Rust's way:** Extism provides **capability-based sandboxing** at the WASM boundary. The plugin can't access anything not explicitly allowed in its manifest. Even if the plugin code is malicious, it's trapped in a memory-safe sandbox with no host access.

✨ **Where Rust shines:** **Defense in depth without containers.** WASM plugins are lighter than Docker (~KB vs ~MB), start faster (~ms vs ~s), and still provide strong isolation. The manifest is human-readable and auditable — you know exactly what a plugin can do before you install it.

### Plugin Discovery

Plugins live in `~/.kimi/plugins/` as `.wasm` + `.json` pairs:

```
~/.kimi/plugins/
├── HttpRequest.wasm
├── HttpRequest.json
├── GitStatus.wasm
└── GitStatus.json
```

At startup, `discover_plugins()` scans this directory and registers every valid plugin as a `Tool`:

```rust
// File: octopus-cli/src/plugin/discovery.rs
pub fn discover_plugins(plugins_dir: &Path) -> Vec<Box<dyn kosong::tooling::CallableTool>> {
    let mut tools = Vec::new();
    for entry in fs::read_dir(plugins_dir).unwrap_or_else(|_| return vec![]) {
        let path = entry.path();
        if path.extension() == Some("wasm".as_ref()) {
            let manifest_path = path.with_extension("json");
            if let Ok(tool) = load_wasm_plugin(&path) {
                tools.push(tool);
            }
        }
    }
    tools
}
```

Both agent-spec-loaded agents **and** basic fallback agents scan for plugins. So even if you don't specify an agent file, your WASM plugins are available.

---

## 🎁 Souvenir Shop: What to Remember

1. **`kosong::CallableTool` is the universal interface.** Every tool — built-in, MCP, WASM plugin, or subagent — speaks the same kosong language: `name()`, `description()`, `parameters()`, `call_raw()`. There is no parallel octopus trait layer.
2. **Execution is middleware-heavy.** Hooks, approval, dedup, and telemetry wrap every tool call. The tool itself only handles the core logic.
3. **Background tasks are first-class.** `ShellTool` can foreground or background with a single flag. The `BackgroundTaskManager` handles the lifecycle.
4. **Subagents are policy-enforced recursive souls.** `AgentTool` looks up types in `LaborMarket`, enforces `ToolPolicy`, resolves model overrides, and shares the parent's runtime via `copy_for_subagent()`.
5. **WASM plugins are sandboxed by default.** Deny-by-default security manifest + Extism runtime = portable, auditable, safe extensions.

---

## 🚶 Next Stop

The Tool Shed has sharp objects. Before the agent uses them, someone needs to approve. Let's visit the **Security Desk**.

→ [Tour 4: The Security Desk](./04-security-desk.md)
