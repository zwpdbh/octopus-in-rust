# 8. Mapping to octopus-cli (Rust)

This section bridges the Python implementation in `tmp/kimi-cli` to the Rust reimplementation in `octopus-cli`. It focuses on **architectural decisions**, **type system improvements**, and **code structure**.

## 8.1 Guiding Principles from AGENTS.md

The `AGENTS.md` file mandates three rules that directly impact the hook system:

1. **Model states with enums and match on them** — never use string literals for event types.
2. **Deserialize JSON into typed structs** — never use `serde_json::Value` and manual indexing.
3. **Use strong enums for channel and IPC messages** — never use `String` or raw bytes as carrier types.

These rules are in direct tension with the Python implementation, which uses:
- `HookEventType = Literal["PreToolUse", ...]` (string literals).
- `dict[str, Any]` payloads built by helper functions.
- `serde_json::Value` in the wire protocol (the Python equivalent is `dict`).

## 8.2 Replace `HookEventType` with a Rust Enum

### Python (old)

```python
HookEventType = Literal[
    "PreToolUse",
    "PostToolUseFailure",
    # ...
]
```

### Rust (new)

```rust
// src/hooks/types.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    UserPromptSubmit,
    Stop,
    StopFailure,
    SessionStart,
    SessionEnd,
    SubagentStart,
    SubagentStop,
    PreCompact,
    PostCompact,
    Notification,
}
```

**Why:**
- Exhaustiveness checking: `match event { ... }` forces handling every variant.
- No typos: `HookEvent::PreToolUse` is compile-time checked.
- Refactoring: rename a variant and the compiler shows every use site.

## 8.3 Replace `dict[str, Any]` with Typed Payloads

### Python (old)

```python
def pre_tool_use(session_id: str, cwd: str, tool_name: str, ...) -> dict[str, Any]:
    return {
        **_base("PreToolUse", session_id, cwd),
        "tool_name": tool_name,
        "tool_input": tool_input,
        "tool_call_id": tool_call_id,
    }
```

### Rust (new)

```rust
// src/hooks/events.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "hook_event_name", rename_all = "PascalCase")]
pub enum HookPayload {
    PreToolUse {
        session_id: String,
        cwd: String,
        tool_name: String,
        tool_input: HashMap<String, serde_json::Value>,
        tool_call_id: String,
    },
    PostToolUseFailure {
        session_id: String,
        cwd: String,
        tool_name: String,
        tool_input: HashMap<String, serde_json::Value>,
        error: String,
        tool_call_id: String,
    },
    // ... every event type
}
```

**Why:**
- Type safety: a `PostToolUseFailure` payload cannot be accidentally passed where `PreToolUse` is expected.
- serde: generates the exact same JSON as the old `dict` builders.
- Adding a field is a compile error at every construction site until it is provided.

## 8.4 Replace `WireEvent` String Dispatch with Strong Enum

### Python (old)

```python
# Wire events are Pydantic models sent as dicts
wire_send(TextPart(text="hello"))
wire_send(TurnBegin(user_input="hi"))
# Consumer deserializes by trial-and-error
```

### Rust (new)

```rust
// src/wire/types.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireEvent {
    TextPart(TextPart),
    TurnBegin(TurnBegin),
    TurnEnd(TurnEnd),
    HookRequest(HookRequest),
    HookResponse(HookResponse),
    HookTriggered(HookTriggered),
    HookResolved(HookResolved),
    // ...
}
```

Construction is explicit:

```rust
wire_send(WireEvent::HookRequest(HookRequest {
    id: handle.id.clone(),
    subscription_id: sub.id.clone(),
    event: HookEvent::PreToolUse,
    target: matcher_value.to_string(),
    input_data: payload,
}));
```

Consumption is exhaustive:

```rust
match event {
    WireEvent::HookRequest(req) => self.handle_hook_request(req).await,
    WireEvent::HookResponse(resp) => self.resolve_hook_response(resp).await,
    WireEvent::TextPart(text) => self.append_text(text.text),
    // Compiler forces every variant
}
```

## 8.5 The HookEngine in Rust

### Data Structures

```rust
// src/hooks/engine.rs
use std::collections::HashMap;
use tokio::process::Command;

pub struct HookEngine {
    hooks: Vec<HookDef>,
    wire_subs: Vec<WireHookSubscription>,
    by_event: HashMap<HookEvent, Vec<HookDef>>,
    wire_by_event: HashMap<HookEvent, Vec<WireHookSubscription>>,
    pending_wire_hooks: HashMap<String, WireHookHandle>,
    on_wire_hook: Option<Box<dyn Fn(WireHookHandle) -> BoxFuture<'static, ()> + Send + Sync>>,
}

#[derive(Debug, Clone)]
pub struct HookDef {
    pub event: HookEvent,
    pub command: String,
    pub matcher: Option<regex::Regex>,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct HookResult {
    pub action: HookAction,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookAction {
    Allow,
    Block,
}
```

### Trigger Method

```rust
impl HookEngine {
    pub async fn trigger(
        &self,
        event: HookEvent,
        matcher_value: &str,
        input_data: &HookPayload,
    ) -> Vec<HookResult> {
        let matched_hooks = self.match_hooks(event, matcher_value);
        let matched_wire = self.match_wire(event, matcher_value);

        let mut tasks: Vec<JoinHandle<HookResult>> = Vec::new();

        // Server-side hooks
        for hook in matched_hooks {
            let cmd = hook.command.clone();
            let data = input_data.clone();
            let timeout = hook.timeout;
            tasks.push(tokio::spawn(async move {
                run_hook(&cmd, &data, timeout).await
            }));
        }

        // Wire-side hooks
        for sub in matched_wire {
            let handle = WireHookHandle::new(sub.id.clone(), event, matcher_value, input_data.clone());
            let id = handle.id.clone();
            // Store and dispatch...
            tasks.push(tokio::spawn(async move {
                handle.wait().await
            }));
        }

        // Gather with fail-open
        let mut results = Vec::new();
        for task in tasks {
            match task.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(HookResult {
                    action: HookAction::Allow,
                    reason: format!("Hook panicked: {}", e),
                }),
            }
        }

        results
    }
}
```

### Runner

```rust
use tokio::process::Command;
use tokio::time::{timeout, Duration};

pub async fn run_hook(command: &str, input_data: &HookPayload, duration: Duration) -> HookResult {
    let json = serde_json::to_vec(input_data).expect("serialize always succeeds");

    let result = timeout(duration, async {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn failed: {}", e))?;

        let mut stdin = child.stdin.take().expect("piped");
        stdin.write_all(&json).await.map_err(|e| format!("stdin: {}", e))?;
        drop(stdin);

        let output = child.wait_with_output().await.map_err(|e| format!("wait: {}", e))?;
        Ok(output)
    }).await;

    match result {
        Ok(Ok(output)) => parse_hook_output(output),
        Ok(Err(reason)) => HookResult { action: HookAction::Allow, reason },
        Err(_) => HookResult { action: HookAction::Allow, reason: "Hook timed out".to_string() },
    }
}

fn parse_hook_output(output: std::process::Output) -> HookResult {
    if output.status.code() == Some(2) {
        return HookResult {
            action: HookAction::Block,
            reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        };
    }

    if output.status.code() == Some(0) {
        if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            if parsed
                .get("hookSpecificOutput")
                .and_then(|o| o.get("permissionDecision"))
                .and_then(|d| d.as_str())
                == Some("deny")
            {
                return HookResult {
                    action: HookAction::Block,
                    reason: "Denied by hook output".to_string(),
                };
            }
        }
        return HookResult { action: HookAction::Allow, reason: String::new() };
    }

    // Fail-open
    HookResult { action: HookAction::Allow, reason: String::new() }
}
```

## 8.6 PreToolUse in Rust

**File:** `src/soul/toolset.rs` (proposed)

```rust
impl Toolset {
    pub async fn call(&self, tool_call: ToolCall) -> Result<ToolResult, OctopusError> {
        let tool_input = tool_call.arguments.clone();

        // 1. Build payload
        let payload = HookPayload::PreToolUse {
            session_id: get_session_id(),
            cwd: std::env::current_dir()?.to_string_lossy().to_string(),
            tool_name: tool_call.function.name.clone(),
            tool_input,
            tool_call_id: tool_call.id.clone(),
        };

        // 2. Trigger hooks
        let results = self.hook_engine.trigger(
            HookEvent::PreToolUse,
            &tool_call.function.name,
            &payload,
        ).await;

        // 3. Check for blocks
        for result in &results {
            if matches!(result.action, HookAction::Block) {
                return Ok(ToolResult {
                    tool_call_id: tool_call.id,
                    return_value: ToolError {
                        message: result.reason.clone().unwrap_or_else(|| "Blocked by PreToolUse hook".to_string()),
                        brief: "Hook blocked".to_string(),
                    },
                });
            }
        }

        // 4. Execute tool
        let tool = self.tools.get(&tool_call.function.name)
            .ok_or_else(|| OctopusError::UnknownTool(tool_call.function.name.clone()))?;
        tool.run(tool_call.arguments).await
    }
}
```

## 8.7 Configuration (TOML → Rust)

```rust
// src/hooks/config.rs
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct HookConfig {
    pub hooks: Vec<HookDefConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HookDefConfig {
    pub event: HookEvent,   // serde deserializes "PreToolUse" → HookEvent::PreToolUse
    pub command: String,
    #[serde(default)]
    pub matcher: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_timeout() -> u64 {
    30
}
```

## 8.8 Key Differences Summary

| Concern | Python (kimi-cli) | Rust (octopus-cli) |
|---------|-------------------|-------------------|
| **Event type** | `Literal["PreToolUse", ...]` | `enum HookEvent` |
| **Payload** | `dict[str, Any]` | `enum HookPayload` with variant data |
| **Wire carrier** | Pydantic model / dict | `enum WireEvent` with `#[serde(untagged)]` |
| **Subprocess** | `asyncio.create_subprocess_shell` | `tokio::process::Command` |
| **Timeout** | `asyncio.wait_for` | `tokio::time::timeout` |
| **Result** | `HookResult(action="block")` | `HookResult { action: HookAction::Block }` |
| **Aggregation** | `for r in results: if r.action == "block"` | `results.iter().any(|r| matches!(r.action, HookAction::Block))` |
| **Fail-open** | `except: return HookResult("allow")` | `Err(_) => HookResult { action: Allow, ... }` |

## 8.9 Testing Strategy

When porting, preserve these test cases:

| Test | Python File | Rust Equivalent |
|------|-------------|-----------------|
| Matching by regex | `tests/hooks/test_engine.py` | `hooks::engine::tests::match_hooks` |
| Blocking aggregation | `tests/hooks/test_engine.py` | `hooks::engine::tests::block_wins` |
| Deduplication | `tests/hooks/test_engine.py` | `hooks::engine::tests::dedup_commands` |
| Exit code 2 blocks | `tests/hooks/test_runner.py` | `hooks::runner::tests::exit_code_two` |
| JSON deny blocks | `tests/hooks/test_runner.py` | `hooks::runner::tests::json_deny` |
| Timeout fail-open | `tests/hooks/test_runner.py` | `hooks::runner::tests::timeout_allow` |
| Wire e2e | `tests/e2e/test_hooks_wire_e2e.py` | `tests::wire_hooks_e2e` |

## 8.10 Common Pitfalls

1. **Shell injection**: `Command::new("sh").arg("-c").arg(command)` is just as vulnerable as `create_subprocess_shell`. Validate `command` comes from a trusted config file.
2. **Regex compilation**: In Rust, compile `matcher` regexes once at startup (in `HookDef`), not on every trigger.
3. **Clone overhead**: `HookPayload` may be large. Consider `Arc<HookPayload>` if profiling shows clone cost matters.
4. **Wire handle leaks**: Ensure `_pending_wire_hooks` entries are removed even if the client never responds.
5. **JSON-RPC ordering**: Wire responses must use the same `id` as the request. Use a `HashMap<String, oneshot::Sender<HookResult>>` or similar.
