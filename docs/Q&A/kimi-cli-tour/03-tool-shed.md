# Tour 3: The Tool Shed — Where Action Happens

> *"The Control Room decides. The Tool Shed executes. Every file read, every shell command, every web search starts here."*

Welcome to the **Tool Shed** — the west wing of the second floor. If the Control Room (`KimiSoul`) is the brain, the Tool Shed is the **hands**. This is where the agent interacts with the outside world: reading files, running shell commands, searching the web, and more.

In this tour, we'll examine the **Tool trait**, the **tool registry**, the **execution pipeline**, and a few star tools.

---

## 🔧 The Universal Wrench: The `Tool` Trait

File: `octopus-cli/src/tools/mod.rs` (~80 lines)

Every tool in the shed implements a single trait:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;  // JSON Schema
    async fn call(&self, arguments: Value) -> ToolReturnValue;
}
```

🐍 **Python's way:** Tools are classes inheriting from a base `Tool` class. Schema is built via Pydantic models or `@property` methods.

🦀 **Rust's way:** A trait with `async_trait`. The `Send + Sync` bounds mean tools can be shared across threads — critical for concurrent execution.

✨ **Where Rust shines:** **Schema is code.** The `parameters_schema()` method returns a `serde_json::Value` — a JSON Schema object built at runtime. But because it's a method call (not a class attribute), you can **generate schemas dynamically**. For example, `McpTool` fetches its schema from the MCP server at connection time:

```rust
impl Tool for McpTool {
    fn parameters_schema(&self) -> Value {
        self.input_schema.clone()  // From MCP server!
    }
}
```

---

## 🗃️ The Tool Registry: `KimiToolset`

File: `octopus-cli/src/soul/toolset.rs` (~777 lines)

The `KimiToolset` is the **shed's inventory system**. It knows every tool and handles execution:

```rust
pub struct KimiToolset {
    tools: Vec<Box<dyn Tool>>,
    tool_map: HashMap<String, usize>,  // name → index
    hook_engine: HookEngine,
    approval: Option<Approval>,
    mcp_servers: HashMap<String, McpServerInfo>,
    // ...
}
```

### Registration

In Python, tools were hardcoded in `KimiSoul.__init__`. In the Rust rewrite, tools are **loaded from the agent spec**:

```rust
// In soul/agent.rs::load_agent()
let spec = load_agent_spec(agent_file)?;
let mut toolset = KimiToolset::new();

for tool_name in spec.tools {
    if let Some(tool) = build_tool(&tool_name, &runtime) {
        toolset.register(tool);
    }
}
```

`build_tool()` maps Python-style names (`kimi_cli.tools.shell:Shell`) to Rust constructors. The agent spec drives the toolbox — no hardcoded list.

🐍 **Python's way:** Tools are registered via `toolset.register_tool(MyTool())` in a list comprehension or loop. Agent spec parsing + dynamic import via `importlib`.

🦀 **Rust's way:** Static name-to-constructor mapping in `build_tool()`. No dynamic imports — the compiler ensures every mapped tool exists. `Box<dyn Tool>` is a **fat pointer** (data + vtable), but lookup is O(1) via `HashMap<String, usize>`.

✨ **Where Rust shines:** **Agent-driven toolset.** Change `agents/default/agent.yaml` and the soul boots with a different toolbox — no recompilation needed for config changes, but the compiler still validates every tool constructor at build time.

---

## ⚡ The Execution Pipeline

When the LLM requests a tool, this pipeline runs:

```rust
pub async fn handle(&self, tool_call: &WireToolCall) -> ToolResult {
    // 1. Find the tool
    let tool = self.tools.get(name)?;

    // 2. Run PreToolUse hook
    if let HookAction::Block { reason } = self.run_pre_hook(...).await {
        return ToolResult::error(format!("Blocked by hook: {}", reason));
    }

    // 3. Request approval (if not yolo/afk)
    if let Some(approval) = &self.approval {
        match approval.request(...).await {
            ApprovalResponse::Approve => {}
            ApprovalResponse::Reject { feedback } => {
                return ToolResult::error(format!("User rejected: {}", feedback));
            }
            // ...
        }
    }

    // 4. Execute!
    let start = Instant::now();
    let result = tool.call(arguments).await;
    let elapsed = start.elapsed();

    // 5. Run PostToolUse hook
    self.run_post_hook(...).await;

    // 6. Track telemetry
    track!("tool_call", tool_name = name, duration_ms = ...);

    result
}
```

This pipeline has **four guardrails** before the tool actually runs:
1. **Hook check** — can the tool run in this context?
2. **Approval check** — does the user want to allow this?
3. **Execution** — the actual work
4. **Post-hook + telemetry** — cleanup and logging

🐍 **Python's way:** Similar pipeline, but hooks and approval are mixed into the tool class hierarchy.

🦀 **Rust's way:** The pipeline is **orthogonal** to tools. Every tool gets the same guards automatically. The `Tool` trait only defines `call()` — everything else is `KimiToolset` middleware.

---

## 🌟 Star Tool: `ShellTool`

File: `octopus-cli/src/tools/shell/mod.rs` (~169 lines)

The `ShellTool` lets the agent run shell commands. It's one of the most powerful — and dangerous — tools.

```rust
pub struct ShellTool {
    bg_manager: BackgroundTaskManager,
}

async fn call(&self, arguments: Value) -> ToolReturnValue {
    let command = arguments["command"].as_str().unwrap();
    let run_in_background = arguments["run_in_background"].as_bool().unwrap_or(false);

    if run_in_background {
        let id = self.bg_manager.spawn(command.to_string(), ...).await?;
        ToolReturnValue::ok_text(format!("Task started: {}", id))
    } else {
        let output = run_command(command).await?;
        ToolReturnValue::ok_text(output)
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

File: `octopus-cli/src/tools/agent/mod.rs` (~160 lines)

The `AgentTool` is the **most mind-bending tool** in the shed. It creates a **new `KimiSoul`** — a recursive agent call!

```rust
async fn run_subagent(config, llm, approval, work_dir, prompt) -> Result<String, String> {
    let session = Session::create(&work_dir, None).await?;
    let mut soul = KimiSoul::new(config, session, llm, approval);
    soul.run(prompt).await
        .map_err(|e| format!("Subagent failed: {}", e))
}
```

When the main agent calls `AgentTool`, it spawns a **fresh soul** with:
- A brand-new session (isolated context)
- The same LLM configuration
- The same approval settings
- The same working directory

🐍 **Python's way:** Subagents are managed by a `SubagentStore` with registry, builder, and output formatting.

🦀 **Rust's way:** A simple async function that creates a `KimiSoul` and calls `run()`. No registry, no builder — just **nested agent loops**.

✨ **Where Rust shines:** **Stack safety.** Recursive async calls in Rust are bounded by the Tokio runtime's stack. In Python, deep recursion can hit the C stack limit or cause `RecursionError`. Rust's async functions are **state machines** compiled into structs — no stack growth per call.

---

## 🔌 The MCP Adapter: External Tools

File: `octopus-cli/src/mcp/client.rs` (~330 lines)

The Tool Shed has a special door for **external tools** — MCP (Model Context Protocol) servers. These are separate processes that expose tools via JSON-RPC over stdio.

```rust
let client = McpClient::connect_stdio("npx", vec!["-y", "@modelcontextprotocol/server-filesystem", "/home"], None).await?;
let tools = client.list_tools().await?;
```

The MCP client:
1. Spawns the server process
2. Performs an `initialize` handshake
3. Calls `tools/list` to discover available tools
4. Wraps each remote tool as a local `McpTool` implementing the `Tool` trait

🐍 **Python's way:** `mcp` Python SDK handles transport and schema conversion.

🦀 **Rust's way:** Hand-rolled JSON-RPC over stdio. ~330 lines total — smaller than many SDK wrappers. Why hand-rolled? Because `rust-mcp-sdk` had tokio compatibility concerns, and our custom client fits our `Tool` trait exactly.

---

## 🎁 Souvenir Shop: What to Remember

1. **The `Tool` trait is the universal interface.** Every tool — built-in, MCP, or future plugin — speaks the same language: `name()`, `description()`, `parameters_schema()`, `call()`.
2. **Execution is middleware-heavy.** Hooks, approval, dedup, and telemetry wrap every tool call. The tool itself only handles the core logic.
3. **Background tasks are first-class.** `ShellTool` can foreground or background with a single flag. The `BackgroundTaskManager` handles the lifecycle.
4. **Subagents are recursive souls.** `AgentTool` creates a new `KimiSoul` — the same brain, fresh memory. This is agentic recursion made simple.

---

## 🚶 Next Stop

The Tool Shed has sharp objects. Before the agent uses them, someone needs to approve. Let's visit the **Security Desk**.

→ [Tour 4: The Security Desk](./04-security-desk.md)
