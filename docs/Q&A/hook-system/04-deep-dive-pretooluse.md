# 4. Deep Dive: How `PreToolUse` Works

Have you ever used an AI assistant and seen something like this?

```
🤖 Thinking...

╭─ approval ───────────────────────────────────────────────────────╮
│  Action:  StrReplaceFile                                         │
│  Tool:    call_abc123                                            │
│                                                                  │
│  Description:                                                    │
│  StrReplaceFile({"path": "src/main.rs", ...})                    │
│                                                                  │
│  [Y] Approve once    [A] Approve for session    [N] Reject       │
╰──────────────────────────────────────────────────────────────────╯
```

You asked the agent to edit a file. The LLM decided to call `StrReplaceFile`. A moment later, an approval dialog appeared asking you to confirm.

**But something invisible happened in between.**

Before that approval dialog ever reached your screen, the system ran the **`PreToolUse` hook**. This hook is a hidden gatekeeper: it can inspect the tool call, run custom scripts, talk to external wire clients, and even **veto the operation entirely** — all before you see the approval prompt.

This chapter traces the full journey from the user-visible approval dialog all the way down to the Rust code that triggers the hook.

---

## 4.1 What You See: The Timeline of a Tool Call

Let's trace the timeline from the user's perspective:

| Time | What you see | What the system is doing |
|------|-------------|--------------------------|
| T+0 | You type "edit main.rs to add logging" | `KimiSoul` starts a new turn |
| T+1 | `🤖 Thinking...` appears | The LLM is generating a response |
| T+2 | *(nothing yet)* | LLM outputs a **tool call** request for `StrReplaceFile` |
| T+3 | *(nothing yet)* | `PreToolUse` hook fires — invisible to you |
| T+4 | *(nothing yet)* | Approval request is broadcast to the UI |
| T+5 | Approval dialog appears | UI is waiting for your [Y]/[N]/[A] |
| T+6 | You press `Y` | Tool finally executes |
| T+7 | File is modified | `PostToolUse` hook fires — also invisible |

The `PreToolUse` hook is the **first gate** in this chain. If it says "block," the tool never runs, the approval dialog never appears, and the agent receives an error instead.

---

## 4.2 Layer 0: The UI — Where the Approval Dialog Comes From

**File:** `octopus-cli/src/ui/shell/mod.rs`

In shell mode, the UI runs an event loop that listens for messages from the "wire" — an internal broadcast channel that carries events like `ApprovalRequest`, `TextPart`, and `HookResolved`.

```rust
// octopus-cli/src/ui/shell/mod.rs ~line 201 — ShellUI event loop (wire event receiver)
Ok(event) = async {
    if let Some(rx) = self.wire_hub_receiver.as_mut() {
        rx.recv().await
    } else {
        std::future::pending().await
    }
} => {
    match event {
        crate::wire::WireEvent::ApprovalRequest(req) => {
            self.pending_approval = Some(req);  // ← Dialog appears!
        }
        // ...
    }
}
```

When the UI receives `ApprovalRequest`, it stores it in `self.pending_approval`. The next time the screen redraws, an overlay appears:

```rust
// octopus-cli/src/ui/shell/mod.rs ~line 1056 — ShellUI::draw_approval_overlay
fn draw_approval_overlay(&self, frame: &mut Frame, pending: &ApprovalRequestEvent) {
    // Renders: Action, Tool, Description, [Y] / [N] / [A]
}
```

**But where did `ApprovalRequest` come from?** To answer that, we go one layer deeper.

---

## 4.3 Layer 1: The Approval System — Broadcasting the Request

**Files:** `octopus-cli/src/soul/approval.rs`, `approval_runtime/runtime.rs`

Inside the tool execution code, after the `PreToolUse` hook passes, the system checks whether the tool requires explicit user approval. `StrReplaceFile` is one of four tools that do:

```rust
// octopus-cli/src/soul/toolset.rs ~line 126 — KimiToolset::requires_approval (private associated fn)
fn requires_approval(name: &str) -> bool {
    matches!(name, "Shell" | "WriteFile" | "StrReplaceFile" | "Agent")
}
```

If approval is required, the toolset calls `approval.request(...)`:

```rust
// octopus-cli/src/soul/toolset.rs ~line 340 — KimiToolset::handle_inner (approval check)
let approval_opt = self.approval.lock().unwrap().clone();
if let Some(ref approval) = approval_opt {
    if Self::requires_approval(&tool_call.function.name) {
        let description = format!("{}({})", tool_call.function.name, args_str);
        let result = approval
            .request("Octopus", &tool_call.function.name, &description, None)
            .await;
        // If rejected, return error ToolResult immediately
    }
}
```

The `Approval::request` method creates a unique request ID and delegates to the `ApprovalRuntime`:

```rust
// octopus-cli/src/approval_runtime/runtime.rs ~line 110 — ApprovalRuntime::create_request
pub fn create_request(...) {
    let req = ApprovalRequest { ... };

    // Publish to the RootWireHub so ALL subscribers (including the Shell UI) receive it
    {
        let inner = self.inner.lock().unwrap();
        if let Some(hub) = inner.hub.as_ref() {
            let event = ApprovalRequestEvent { ... };
            hub.publish(WireEvent::ApprovalRequest(event));
        }
    }

    inner.requests.insert(request_id, req);
}
```

`RootWireHub` is a `tokio::sync::broadcast` channel. When something is published, every subscriber — including the Shell UI — receives a copy. That's how the approval dialog travels from the tool execution thread to your screen.

**But we skipped a step.** Before `approval.request()` is called, something else already happened. Let's go deeper.

---

## 4.4 Layer 2: The Toolset — Where the Hook Lives

**File:** `octopus-cli/src/soul/toolset.rs`

When the LLM decides to call a tool, `kosong` (the inference engine) invokes the toolset. `KimiToolsetHandle` wraps an `Arc<KimiToolset>` and spawns the real work into a `tokio::task`:

```rust
// octopus-cli/src/soul/toolset.rs ~line 720 — KimiToolsetHandle::handle
impl kosong::Toolset for KimiToolsetHandle {
    fn handle(&self, tool_call: &kosong::ToolCall) -> kosong::HandleResult {
        let inner = std::sync::Arc::clone(&self.0);
        let tc = tool_call.clone();
        let handle = tokio::spawn(async move { inner.handle_inner(&tc).await });
        kosong::HandleResult::Pending(handle)
    }
}
```

`handle_inner` is the heart of tool execution. Here is its control flow, stripped to the essentials:

```
handle_inner(tool_call)
  │
  ├── 1. Same-step dedup check (wait for identical call in same step)
  │
  ├── 2. Cross-step dedup check (warn if repeated from previous step)
  │
  ├── 3. Parse JSON arguments
  │
  ├── 4. >>> PRETOOLUSE HOOK <<<  ← YOU ARE HERE
  │      │
  │      ├── Build HookEvent::PreToolUse payload
  │      ├── Call hook_engine.trigger(event, tool_name)
  │      └── If any hook returns Block → return error immediately
  │
  ├── 5. Approval check  ← Only if PreToolUse did NOT block
  │      │
  │      ├── Is tool in requires_approval list?
  │      ├── If yes: broadcast ApprovalRequest, wait for user
  │      └── If rejected → return error immediately
  │
  ├── 6. Execute the tool (call_raw)
  │
  └── 7. PostToolUse / PostToolUseFailure hook (fire-and-forget)
```

The `PreToolUse` hook is **step 4** — the very first gate after parsing arguments. It runs **before** deduplication? No wait, dedup is first. But the hook runs before approval and before execution.

Here is the actual code for step 4:

```rust
// octopus-cli/src/soul/toolset.rs ~line 306 — KimiToolset::handle_inner (PreToolUse hook)
// --- PreToolUse hook ---
let tool_input_map = match arguments.as_object() {
    Some(obj) => obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    None => std::collections::HashMap::new(),
};

let event = HookEvent::pre_tool_use(
    &self.session_id,
    &self.cwd,
    &tool_call.function.name,
    &tool_input_map,
    &tool_call.id,
);

// Fast path: skip all work if no hooks are registered for this event
if self.hook_engine.has_hooks_for(&event) {
    let results = self
        .hook_engine
        .trigger(event, &tool_call.function.name)
        .await;

    for r in &results {
        if let crate::hooks::runner::HookAction::Block(ref reason) = r.action {
            // Hook blocked! Return error without running the tool.
            let result = kosong::tooling::ToolResult {
                tool_call_id: tool_call.id.clone(),
                return_value: kosong::tooling::ToolReturnValue::error(reason.clone()),
            };
            // Track in step state for deduplication
            let mut state = self.step_state.lock().unwrap();
            state.current_step_results.insert(call_key.clone(), result.clone());
            state.current_step_calls.push(call_key);
            return result;
        }
    }
}
```

Notice the `has_hooks_for` guard. If no hooks are registered for `PreToolUse`, the engine skips serialization and task spawning entirely — a zero-cost fast path.

---

## 4.5 Layer 3: The Payload — What Data Does the Hook Receive?

**File:** `octopus-cli/src/hooks/event.rs`

The hook receives a fully-typed Rust enum that serializes to JSON. Here is the definition:

```rust
// octopus-cli/src/hooks/event.rs ~line 13 — HookEvent enum definition
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "hook_event_name", rename_all = "PascalCase")]
pub enum HookEvent {
    PreToolUse {
        session_id: String,
        cwd: String,
        tool_name: String,
        tool_input: HashMap<String, Value>,
        tool_call_id: String,
    },
    // ... other variants
}
```

The helper constructor builds the payload:

```rust
// octopus-cli/src/hooks/event.rs ~line 109 — HookEvent::pre_tool_use helper
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

For a `StrReplaceFile` call, the JSON that gets piped into the hook script's stdin looks like this:

```json
{
  "hook_event_name": "PreToolUse",
  "session_id": "sess_abc123",
  "cwd": "/home/user/octopus",
  "tool_name": "StrReplaceFile",
  "tool_input": {
    "path": "src/main.rs",
    "old_string": "fn main() {",
    "new_string": "fn main() {\n    println!(\"Starting...\");"
  },
  "tool_call_id": "call_xyz789"
}
```

**Key design decision:** `HookEvent` implements `PartialEq` and `Hash` using **only the discriminant** (the variant name), not the payload data. This means a config hook registered for `PreToolUse` matches **any** runtime `PreToolUse` event, regardless of `tool_name`, `session_id`, etc. The regex `matcher` field is what filters by tool name.

---

## 4.6 Layer 4: The Engine — Matching and Running Hooks

**File:** `octopus-cli/src/hooks/engine.rs`

The `HookEngine` is the central dispatcher. It maintains two indexes:

- `by_event: HashMap<HookEvent, Vec<HookDef>>` — server-side hooks from `config.toml`
- `wire_by_event: HashMap<HookEvent, Vec<WireHookSubscription>>` — client-side hooks from wire `initialize`

When `trigger()` is called, the engine does six things:

### Step 1: Match server-side hooks

```rust
// octopus-cli/src/hooks/engine.rs ~line 200 — HookEngine::trigger (Step 1: match server-side hooks)
let mut seen_commands: std::collections::HashSet<String> = std::collections::HashSet::new();
let mut server_matched: Vec<&HookDef> = Vec::new();
for h in self.by_event.get(&*event).into_iter().flatten() {
    if !Self::match_regex(
        h.compiled_matcher.as_ref(),
        h.matcher.as_deref().unwrap_or(""),
        matcher_value,
    ) {
        continue;
    }
    if seen_commands.contains(&h.command) {
        continue;  // skip duplicate commands
    }
    seen_commands.insert(h.command.clone());
    server_matched.push(h);
}
```

### Step 2: Match wire subscriptions

```rust
// octopus-cli/src/hooks/engine.rs ~line 218 — HookEngine::trigger (Step 2: match wire subscriptions)
let wire_matched: Vec<&WireHookSubscription> = self
    .wire_by_event
    .get(&*event)
    .into_iter()
    .flatten()
    .filter(|s| Self::match_regex(s.compiled_matcher.as_ref(), &s.matcher, matcher_value))
    .collect();
```

### Step 3: Emit triggered callback

```rust
// octopus-cli/src/hooks/engine.rs ~line 232 — HookEngine::trigger (Step 3: emit triggered callback)
if let Some(ref cb) = self.on_triggered {
    cb(&*event, matcher_value, total);
}
```

This broadcasts `WireEvent::HookTriggered` so GUI clients can show "Running hooks..."

### Step 4: Run everything in parallel

```rust
// octopus-cli/src/hooks/engine.rs ~line 237 — HookEngine::trigger (Step 4: run everything in parallel)
let t0 = std::time::Instant::now();
let mut tasks: Vec<tokio::task::JoinHandle<HookResult>> = Vec::new();

// Server hooks: spawn subprocesses
for h in server_matched {
    let command = h.command.clone();
    let event = Arc::clone(&event);
    let timeout = h.timeout;
    let cwd = self.cwd.clone();
    tasks.push(tokio::spawn(async move {
        run_hook(&command, &*event, timeout, cwd.as_deref()).await
    }));
}

// Wire hooks: dispatch to external client
for s in wire_matched {
    // ... create WireHookHandle, send request, await response ...
}
```

### Step 5: Aggregate results

```rust
// octopus-cli/src/hooks/engine.rs ~line 305 — HookEngine::trigger (Step 5: aggregate results)
let results: Vec<HookResult> = match futures::future::try_join_all(tasks).await {
    Ok(r) => r,
    Err(e) => {
        tracing::warn!("Hook engine task join error for {}: {}", event, e);
        return Vec::new();
    }
};
```

If any task panics or fails to join, the engine logs a warning and returns empty (fail-open).

### Step 6: Block wins

```rust
// octopus-cli/src/hooks/engine.rs ~line 315 — HookEngine::trigger (Step 6: block wins, emit resolved)
let mut action = HookAction::Allow;
for r in &results {
    if let HookAction::Block(ref reason) = r.action {
        action = HookAction::Block(reason.clone());
        tracing::warn!("Hook blocked {} (matcher={}): {}", event, matcher_value, reason);
        break;
    }
}

if let Some(ref cb) = self.on_resolved {
    cb(&event, matcher_value, action, duration_ms);
}
```

This broadcasts `WireEvent::HookResolved` so the UI can display "Hook blocked PreToolUse: ..."

---

## 4.7 Layer 5: The Runner — How a Shell Hook Actually Executes

**File:** `octopus-cli/src/hooks/runner.rs`

Each server-side hook is a shell command. The runner:

1. Serializes the `HookEvent` to JSON bytes
2. Spawns `sh -c "<command>"` with piped stdin/stdout/stderr
3. Writes the JSON to the child's stdin
4. Closes stdin (so the child sees EOF)
5. Waits for output with a timeout

```rust
// octopus-cli/src/hooks/runner.rs ~line 61 — run_hook
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

    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Hook failed to spawn: {}: {}", command, e);
            return HookResult::allow();
        }
    };

    // ... write stdin, wait with timeout ...
}
```

### The Decision Protocol

After the subprocess exits, the runner interprets the result:

```rust
// octopus-cli/src/hooks/runner.rs ~line 134 — run_hook (decision logic)
let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
let exit_code = output.status.code().unwrap_or(0);

// Exit 2 = block (reason from stderr)
if exit_code == 2 {
    return HookResult {
        action: HookAction::Block(stderr.trim().to_string()),
        stdout,
        stderr,
        exit_code: 2,
        timed_out: false,
    };
}

// Exit 0 + JSON stdout = structured decision
if exit_code == 0 && !stdout.trim().is_empty() {
    if let Ok(parsed) = serde_json::from_str::<HookStdout>(&stdout) {
        if let Some(ref output) = parsed.hook_specific_output {
            if output.permission_decision.as_deref() == Some("deny") {
                let reason = output.permission_decision_reason.clone().unwrap_or_default();
                return HookResult {
                    action: HookAction::Block(reason),
                    stdout,
                    stderr,
                    exit_code: 0,
                    timed_out: false,
                };
            }
        }
    }
}

// Everything else = allow (fail-open)
HookResult {
    action: HookAction::Allow,
    stdout,
    stderr,
    exit_code,
    timed_out: false,
}
```

**Why the manual struct construction?** The runner preserves `stdout`, `stderr`, and `exit_code` even for `Allow` outcomes, so telemetry can log what the hook said.

---

## 4.8 Layer 6: The Wire-Side Path — Client-Side Hooks

**File:** `octopus-cli/src/wire_server/mod.rs`

If a wire client (like a VS Code extension) has subscribed to `PreToolUse`, the engine creates a `WireHookHandle` and dispatches it through a callback:

```rust
// octopus-cli/src/wire_server/mod.rs ~line 441 — WireServer::handle_prompt (wire hook callback)
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

The client receives a JSON-RPC request:

```json
{
  "jsonrpc": "2.0",
  "id": "uuid-handle",
  "method": "HookRequest",
  "params": {
    "id": "uuid-handle",
    "subscription_id": "sub1",
    "event": "PreToolUse",
    "target": "StrReplaceFile",
    "input_data": { ... }
  }
}
```

And responds with:

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

The `WireServer` routes the response back to the pending `WireHookHandle`, which resolves the oneshot channel and unblocks the engine.

---

## 4.9 Back to the Tool: Block or Execute?

**File:** `octopus-cli/src/soul/toolset.rs`

After `hook_engine.trigger()` returns, `handle_inner` checks the results:

```rust
// octopus-cli/src/soul/toolset.rs ~line 324 — KimiToolset::handle_inner (check hook results)
for r in &results {
    if let crate::hooks::runner::HookAction::Block(ref reason) = r.action {
        // Hook blocked! Build error result.
        let result = kosong::tooling::ToolResult {
            tool_call_id: tool_call.id.clone(),
            return_value: kosong::tooling::ToolReturnValue::error(reason.clone()),
        };
        // Store in step state so same-step deduplication can reuse it
        let mut state = self.step_state.lock().unwrap();
        state.current_step_results.insert(call_key.clone(), result.clone());
        state.current_step_calls.push(call_key);
        return result;
    }
}

// No block — proceed to approval check, then execute the tool
```

If no hook blocked, execution continues to the approval gate, and finally to `tool.call_raw(arguments)`.

---

## 4.10 Complete Sequence Diagram

Here is the full journey from LLM decision to user screen:

```mermaid
sequenceDiagram
    participant U as User
    participant S as Shell UI
    participant H as RootWireHub
    participant A as ApprovalRuntime
    participant T as KimiToolset
    participant E as HookEngine
    participant R as run_hook()
    participant W as WireServer
    participant C as Client

    U->>S: "edit main.rs to add logging"
    S-->>U: 🤖 Thinking...

    Note over T: LLM decides to call StrReplaceFile
    T->>E: build HookEvent::PreToolUse
    activate E
    E->>R: spawn subprocess (server hook)
    activate R
    R-->>E: exit 0 → Allow
    deactivate R

    E->>W: dispatch wire hook handle
    activate W
    W->>C: JSON-RPC HookRequest
    activate C
    C-->>W: response → Allow
    deactivate C
    W-->>E: HookResult(Allow)
    deactivate W

    E-->>T: Vec&lt;HookResult&gt; → all Allow
    deactivate E

    T->>A: approval.request()
    activate A
    A->>H: publish ApprovalRequest
    activate H
    H->>S: ApprovalRequest event
    deactivate H
    S-->>U: draw overlay [Y] [N] [A]

    U->>S: press Y
    S->>H: resolve
    activate H
    H-->>A: wait_for_response → Allow
    deactivate H
    A-->>T: ApprovalResult::Allow
    deactivate A

    T->>T: tool.call_raw(arguments)
    activate T
    T-->>T: ToolReturnValue
    T->>T: fire_and_forget_trigger(PostToolUse)
    deactivate T
```

---

## 4.11 Key Invariants

1. **Block wins over allow**: Even if 9 hooks say `allow` and 1 says `block`, the tool is blocked.
2. **Fail-open**: A crashed hook, timeout, or malformed response is treated as `allow`.
3. **Parallel, not serial**: All matched hooks run simultaneously; total latency is the slowest hook, not the sum.
4. **Regex filtering**: A hook only runs if its `matcher` regex matches the `matcher_value` (tool name for `PreToolUse`). The regex is compiled **once** at config load time.
5. **No leaks**: Wire hook handles are removed from `pending_requests` whether the client responds, drops, or times out.
6. **Fast path**: `has_hooks_for` checks the index before serializing or spawning anything. If no hooks match, `trigger` is essentially free.
7. **Hook before approval**: `PreToolUse` runs **before** the approval dialog. A blocked hook means the user never sees the approval request.

---

## 4.12 Python Comparison

The Python version was structurally identical but differed in key details:

| Aspect | Python (`tmp/kimi-cli`) | Rust (`octopus-cli`) |
|--------|------------------------|----------------------|
| Payload | Plain `dict[str, Any]` | `HookEvent` enum with `Serialize` |
| Index | `dict[str, list[HookDef]]` | `HashMap<HookEvent, Vec<HookDef>>` |
| Regex | Compiled on every trigger | Compiled once at config load |
| Equality | String comparison (`event == "PreToolUse"`) | Discriminant-only `PartialEq` |
| Wire cleanup | Leaked until GC | `on_wire_hook_done` callback |
| Fast path | Always serialized | `has_hooks_for` guard |

The JSON protocol sent to hook scripts is identical, so shell hooks written for the Python version work unchanged in the Rust version.
