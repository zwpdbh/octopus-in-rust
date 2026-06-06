# 4. Deep Dive: How `PreToolUse` Works

`PreToolUse` is the most critical hook because it is **blocking**: it can prevent a tool from executing entirely. This section traces every line of Rust code from the moment a tool is requested to the moment it either runs or is vetoed.

## 4.1 The Trigger Site

**File:** `octopus-cli/src/soul/toolset.rs`

When the LLM requests a tool call, `Toolset::call()` builds the payload and passes it to the `HookEngine`:

```rust
impl Toolset {
    pub async fn call(&self, tool_call: ToolCall) -> Result<ToolResult> {
        let arguments = parse_arguments(&tool_call.function.arguments)?;

        // ============================================
        // 1. BUILD THE PAYLOAD (typed enum variant)
        // ============================================
        let event = HookEvent::pre_tool_use(
            get_session_id(),
            std::env::current_dir()?.to_string_lossy(),
            &tool_call.function.name,
            &arguments,
            &tool_call.id,
        );

        // ============================================
        // 2. TRIGGER HOOKS (blocking wait)
        // ============================================
        let results = self.hook_engine.trigger(event, &tool_call.function.name).await;

        // ============================================
        // 3. CHECK THE AGGREGATED RESULT
        // ============================================
        for result in &results {
            if let HookAction::Block(ref reason) = result.action {
                return Ok(ToolResult {
                    tool_call_id: tool_call.id,
                    return_value: ToolReturnValue::error(
                        reason.clone(),
                        "Hook blocked".to_string(),
                        None,
                    ),
                });
            }
        }

        // ============================================
        // 4. EXECUTE THE TOOL (only if allowed)
        // ============================================
        let tool = self.tools.get(&tool_call.function.name)
            .ok_or_else(|| OctopusError::UnknownTool(tool_call.function.name.clone()))?;
        tool.run(arguments).await
    }
}
```

This is a **synchronous wait**: `await` pauses the tool execution task until every matched hook handler returns.

**Python comparison:** The Python version was structurally identical but used a plain `dict` for the payload:

```python
results = await self._hook_engine.trigger(
    "PreToolUse",
    matcher_value=tool_call.function.name,
    input_data=events.pre_tool_use(
        session_id=_get_session_id(),
        cwd=str(Path.cwd()),
        tool_name=tool_call.function.name,
        tool_input=tool_input_dict,
        tool_call_id=tool_call.id,
    ),
)
```

## 4.2 The Payload

**File:** `octopus-cli/src/hooks/event.rs`

```rust
impl HookEvent {
    pub fn pre_tool_use(
        session_id: impl Into<String>,
        cwd: impl Into<String>,
        tool_name: impl Into<String>,
        tool_input: &HashMap<String, Value>,
        tool_call_id: impl Into<String>,
    ) -> Self {
        HookEvent::PreToolUse {
            session_id: session_id.into(),
            cwd: cwd.into(),
            tool_name: tool_name.into(),
            tool_input: tool_input.clone(),
            tool_call_id: tool_call_id.into(),
        }
    }
}
```

For a `shell` tool call with `{"command": "rm -rf /tmp/old"}`, the JSON sent to stdin looks identical to the Python version:

```json
{
  "hook_event_name": "PreToolUse",
  "session_id": "sess_abc123",
  "cwd": "/home/user/project",
  "tool_name": "shell",
  "tool_input": {
    "command": "rm -rf /tmp/old"
  },
  "tool_call_id": "call_xyz789"
}
```

The difference is that in Rust, this JSON is produced by `serde` deriving `Serialize` on the enum, not by a hand-written helper function building a `dict`.

## 4.3 The Engine Trigger Method

**File:** `octopus-cli/src/hooks/engine.rs`

```rust
pub async fn trigger(&self, event: HookEvent, matcher_value: &str) -> Vec<HookResult> {
    let event = Arc::new(event);
    let input_data = serde_json::to_value(&*event).unwrap_or_default();

    // 1. Match server-side hooks by discriminant + regex
    let mut server_matched: Vec<&HookDef> = Vec::new();
    let mut seen_commands = HashSet::new();
    for h in self.by_event.get(&*event).into_iter().flatten() {
        if !Self::match_regex(h.compiled_matcher.as_ref(), h.matcher.as_deref().unwrap_or(""), matcher_value) {
            continue;
        }
        if seen_commands.insert(h.command.clone()) {
            server_matched.push(h);
        }
    }

    // 2. Match wire subscriptions
    let wire_matched: Vec<&WireHookSubscription> = self
        .wire_by_event
        .get(&*event)
        .into_iter()
        .flatten()
        .filter(|s| Self::match_regex(s.compiled_matcher.as_ref(), &s.matcher, matcher_value))
        .collect();

    let total = server_matched.len() + wire_matched.len();
    if total == 0 {
        return Vec::new();
    }

    // 3. Emit triggered callback (for wire telemetry)
    if let Some(ref cb) = self.on_triggered {
        cb(&*event, matcher_value, total);
    }

    // 4. Run everything in parallel
    let t0 = std::time::Instant::now();
    let mut tasks: Vec<JoinHandle<HookResult>> = Vec::new();

    // Server-side: spawn subprocesses
    for h in server_matched {
        let command = h.command.clone();
        let event = Arc::clone(&event);
        let timeout = h.timeout;
        let cwd = self.cwd.clone();
        tasks.push(tokio::spawn(async move {
            run_hook(&command, &*event, timeout, cwd.as_deref()).await
        }));
    }

    // Wire-side: dispatch to client
    let on_done = self.on_wire_hook_done.clone();
    for s in wire_matched {
        if let Some(ref cb) = self.on_wire_hook {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let handle = WireHookHandle {
                id: uuid::Uuid::new_v4().to_string(),
                subscription_id: s.id.clone(),
                event_name: event.to_string(),
                target: matcher_value.to_string(),
                input_data: input_data.clone(),
                tx: Some(tx),
            };
            let handle_id = handle.id.clone();
            let cb_future = cb(handle);
            let on_done = on_done.clone();
            tasks.push(tokio::spawn(async move {
                cb_future.await;
                let result = match tokio::time::timeout(
                    tokio::time::Duration::from_secs(s.timeout), rx
                ).await {
                    Ok(Ok(r)) => r,
                    Ok(Err(_)) => { HookResult::allow() }
                    Err(_) => { HookResult::allow() /* timed out */ }
                };
                if let Some(ref cb) = on_done {
                    cb(&handle_id);
                }
                result
            }));
        }
    }

    let results = futures::future::try_join_all(tasks).await.unwrap_or_default();

    // 5. Aggregate: block wins
    let mut action = HookAction::Allow;
    for r in &results {
        if let HookAction::Block(ref reason) = r.action {
            action = HookAction::Block(reason.clone());
            break;
        }
    }

    // 6. Emit resolved callback
    if let Some(ref cb) = self.on_resolved {
        cb(&*event, matcher_value, action, t0.elapsed().as_millis() as u64);
    }

    results
}
```

### Step-by-step breakdown:

| Step | What happens | Python equivalent |
|------|--------------|-------------------|
| **1. Arc wrap** | `Arc::new(event)` so N hooks share one payload | Python passed dicts by reference, but each async task closure captured its own copy |
| **2. Pre-serialize** | `serde_json::to_value` once for all wire hooks | Python serialized inside each `wire_send` call |
| **3. Discriminant match** | `HashMap<HookEvent, Vec<HookDef>>` lookup | Python iterated a list and compared strings |
| **4. Regex filter** | `Regex::is_match` using **pre-compiled** regex | Python called `re.search(pattern, value)` — compiled on every trigger |
| **5. Deduplicate** | `HashSet<String>` skips duplicate commands | Same approach in Python |
| **6. Parallel run** | `tokio::spawn` + `try_join_all` | `asyncio.create_task` + `asyncio.gather` |
| **7. Cleanup** | `on_wire_hook_done` removes stale `pending_requests` entries | Python had no cleanup — leaked handles until GC |

## 4.4 The Server-Side Runner

**File:** `octopus-cli/src/hooks/runner.rs`

```rust
pub async fn run_hook(
    command: &str,
    event: &HookEvent,
    timeout_secs: u64,
    cwd: Option<&std::path::Path>,
) -> HookResult {
    let json_input = match serde_json::to_vec(event) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Hook failed to serialize event: {}", e);
            return HookResult::allow();
        }
    };

    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();
    // ... write stdin, wait for output
}
```

### Decision Logic

```rust
let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
let exit_code = output.status.code().unwrap_or(0);

// Exit 2 = block (reason from stderr)
if exit_code == 2 {
    return HookResult::block(stderr.trim());
}

// Exit 0 + JSON stdout = structured decision
if exit_code == 0 && !stdout.trim().is_empty() {
    if let Ok(parsed) = serde_json::from_str::<HookStdout>(&stdout) {
        if let Some(ref output) = parsed.hook_specific_output {
            if output.permission_decision.as_deref() == Some("deny") {
                let reason = output.permission_decision_reason.clone().unwrap_or_default();
                return HookResult::block(reason);
            }
        }
    }
}

HookResult::allow()
```

**Python comparison:** The Python runner used the exact same exit-code protocol, but parsed stdout with manual `dict` indexing:

```python
parsed = json.loads(stdout)
if parsed.get("hookSpecificOutput", {}).get("permissionDecision") == "deny":
    ...
```

The Rust version uses typed structs (`HookStdout`, `HookSpecificOutput`) so a typo in a field name is a **compile error**, not a silent `None` at runtime.

### Example Shell Hook Script

Save this as `/home/user/hooks/block_dangerous.py`:

```python
#!/usr/bin/env python3
import sys, json

payload = json.load(sys.stdin)
command = payload.get("tool_input", {}).get("command", "")

if "rm -rf /" in command:
    sys.stderr.write("Blocking dangerous command: rm -rf /")
    sys.exit(2)  # Block!

print(json.dumps({"hookSpecificOutput": {"permissionDecision": "allow"}}))
sys.exit(0)
```

Register it in `config.toml`:

```toml
[[hooks]]
event = "PreToolUse"
command = "python3 /home/user/hooks/block_dangerous.py"
matcher = "shell"
timeout = 5
```

The script works identically in both Python and Rust versions because the JSON protocol and exit-code semantics are preserved.

## 4.5 The Wire-Side Path

If a wire client has subscribed to `PreToolUse`, the engine also creates a `WireHookHandle`.

**File:** `octopus-cli/src/wire_server/mod.rs`

```rust
let on_wire_hook: OnWireHook = Box::new(move |handle: WireHookHandle| {
    let pending = pending.clone();
    let write_tx = write_tx.clone();
    Box::pin(async move {
        let request = HookRequest {
            id: handle.id.clone(),
            subscription_id: handle.subscription_id.clone(),
            event: handle.event_name.clone(),
            target: handle.target.clone(),
            input_data: handle.input_data.clone(),
        };
        let request_id = request.id.clone();
        pending.lock().await.insert(request_id.clone(), PendingRequest::Hook(handle));
        if let Some(ref tx) = write_tx {
            let envelope = JSONRPCRequestMessage::new(request_id, request);
            if let Ok(v) = serde_json::to_value(&envelope) {
                let _ = tx.send(v);
            }
        }
    })
});
```

The client receives:

```json
{
  "jsonrpc": "2.0",
  "id": "uuid-handle",
  "method": "HookRequest",
  "params": {
    "id": "uuid-handle",
    "subscription_id": "sub1",
    "event": "PreToolUse",
    "target": "shell",
    "input_data": { ... }
  }
}
```

The client responds with:

```json
{
  "jsonrpc": "2.0",
  "id": "uuid-handle",
  "result": {
    "request_id": "uuid-handle",
    "action": "block",
    "reason": "User denied this action"
  }
}
```

**Python comparison:** The Python wire server stored `PendingRequest` in a dict and removed it only on client response. If the client disconnected, the entry leaked. The Rust version adds an `on_wire_hook_done` callback that cleans up the entry even on timeout:

```rust
let on_done = Arc::new(move |id: &str| {
    let pending = pending_cleanup.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        pending.lock().await.remove(&id);
    });
});
soul.hook_engine.set_on_wire_hook_done(Some(on_done));
```

## 4.6 Back to the Tool: Block or Execute?

**File:** `octopus-cli/src/soul/toolset.rs`

```rust
for result in &results {
    if let HookAction::Block(ref reason) = result.action {
        return Ok(ToolResult {
            tool_call_id: tool_call.id,
            return_value: ToolReturnValue::error(
                reason.clone(),
                "Hook blocked".to_string(),
                None,
            ),
        });
    }
}

// No block — execute the tool
let tool = self.tools.get(&tool_call.function.name)?;
tool.run(arguments).await
```

## 4.7 Complete Sequence Diagram

```
LLM / Planner          Toolset::call()             HookEngine          run_hook()          WireServer          Client
    │                        │                         │                   │                  │                │
    │── "call shell" ───────▶│                         │                   │                  │                │
    │                        │                         │                   │                  │                │
    │                        │── build event ─────────▶│                   │                  │                │
    │                        │   (HookEvent::pre_tool_use)                 │                  │                │
    │                        │                         │                   │                  │                │
    │                        │── engine.trigger() ────▶│                   │                  │                │
    │                        │                         │                   │                  │                │
    │                        │                         │── match + dedup ──┤                  │                │
    │                        │                         │   (compiled regex)│                  │                │
    │                        │                         │                   │                  │                │
    │                        │                         │── run_hook() ────▶│                  │                │
    │                        │                         │   (server hook)   │── subprocess ────┤                │
    │                        │                         │                   │   (JSON stdin)   │                │
    │                        │                         │                   │                  │                │
    │                        │                         │◀─ exit 2 ─────────│                  │                │
    │                        │                         │   (block)         │                  │                │
    │                        │                         │                   │                  │                │
    │                        │                         │── wire handle ───────────────────────▶│── request ────▶│
    │                        │                         │                   │                  │                │
    │                        │                         │◀──────────────────────────────────────│◀─ response ───│
    │                        │                         │   (or timeout → on_done cleanup)      │                │
    │                        │                         │                   │                  │                │
    │                        │◀─ [HookResult(block)] ──│                   │                  │                │
    │                        │                         │                   │                  │                │
    │                        │── check results ────────┤                   │                  │                │
    │                        │   action == Block       │                   │                  │                │
    │                        │                         │                   │                  │                │
    │◀── ToolError ──────────│                       │                   │                  │                │
    │   "Blocked by hook"    │                       │                   │                  │                │
```

## 4.8 Key Invariants

1. **Block wins over allow**: Even if 9 hooks say `allow` and 1 says `block`, the tool is blocked.
2. **Fail-open**: A crashed hook, timeout, or malformed response is treated as `allow`.
3. **Parallel, not serial**: All matched hooks run simultaneously; total latency is the slowest hook, not the sum.
4. **Regex filtering**: A hook only runs if its `matcher` regex matches the `matcher_value` (tool name for `PreToolUse`). The regex is compiled **once** at config load time.
5. **No leaks**: Wire hook handles are removed from `pending_requests` whether the client responds, drops, or times out.
