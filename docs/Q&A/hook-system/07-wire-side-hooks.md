# 7. Wire-Side Hooks

Wire-side hooks extend the hook system across process boundaries. When a client connects to the CLI via the wire protocol (JSON-RPC over stdio or socket), it can subscribe to events. The server then forwards matching events to the client and waits for a decision.

## 7.1 Why Wire-Side Hooks Exist

Not all hook logic can live in a local shell script:

- **GUI clients** want to show a confirmation dialog before a dangerous tool runs.
- **Remote agents** need to enforce policies from a central server.
- **IDE plugins** want to intercept file writes to update their own state.

Wire-side hooks let these clients participate in the hook system as first-class citizens.

## 7.2 Types

**File:** `src/wire/event.rs`

### HookTriggered / HookResolved

These are fire-and-forget wire events that let a UI observe hook execution:

```rust
// octopus-cli/src/wire/event.rs ~line 252 — HookTriggered / HookResolved
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookTriggered {
    pub event: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub hook_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResolved {
    pub event: String,
    #[serde(default)]
    pub target: String,
    #[serde(default = "default_allow")]
    pub action: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub duration_ms: u64,
}
```

### HookRequest

```rust
// octopus-cli/src/wire/event.rs ~line 275 — HookRequest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRequest {
    pub id: String,
    pub subscription_id: String,
    pub event: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub input_data: serde_json::Value,
}
```

### HookResponse

**File:** `src/wire/jsonrpc.rs`

```rust
// octopus-cli/src/wire/jsonrpc.rs ~line 311 — HookResponse (Deserialize only)
#[derive(Debug, Clone, Deserialize)]
pub struct HookResponse {
    pub request_id: String,
    #[serde(default = "default_allow_action")]
    pub action: String,
    #[serde(default)]
    pub reason: String,
}
```

## 7.3 Registration Flow

```
Client                              WireServer                         HookEngine
  │                                    │                                  │
  │── JSONRPCRequest ─────────────────▶│                                  │
  │   method: "initialize"             │                                  │
  │   params: {                        │                                  │
  │     hooks: [                       │                                  │
  │       {id: "sub1", event: "PreToolUse"}
  │     ]                              │                                  │
  │   }                                │                                  │
  │                                    │                                  │
  │                                    │── engine.add_wire_subscriptions()─▶│
  │                                    │                                  │
  │◀── JSONRPCResponse ────────────────│                                  │
  │   result: {capabilities: {...}}    │                                  │
```

**File:** `src/wire_server/mod.rs`

```rust
// octopus-cli/src/wire_server/mod.rs ~line 340 — Wire hook registration
if let Some(hooks) = msg.params.hooks {
    let mut subs: Vec<crate::hooks::WireHookSubscription> = Vec::new();
    for h in hooks {
        match parse_hook_event(&h.event) {
            Some(event) => subs.push(crate::hooks::WireHookSubscription {
                id: h.id,
                event,
                matcher: h.matcher,
                compiled_matcher: None,
                timeout: h.timeout,
            }),
            None => {
                tracing::warn!("Ignoring unknown hook event from client: {}", h.event);
            }
        }
    }
    if !subs.is_empty() {
        let mut soul = self.soul.lock().await;
        soul.hook_engine.add_wire_subscriptions(subs);
    }
}
```

## 7.4 Trigger Flow

When `HookEngine::trigger()` matches a wire subscription, it creates a `WireHookHandle` and calls the `on_wire_hook` callback:

```rust
// octopus-cli/src/hooks/engine.rs ~line 251 — Wire hook trigger flow
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
let timeout_secs = s.timeout;
let target = matcher_value.to_string();
let cb_future = cb(handle);
let on_done = on_done.clone();
tasks.push(tokio::spawn(async move {
    cb_future.await;
    let result = match tokio::time::timeout(
        tokio::time::Duration::from_secs(timeout_secs),
        rx,
    ).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            tracing::warn!("Wire hook resolver dropped without resolving");
            HookResult::allow()
        }
        Err(_) => {
            tracing::warn!("Wire hook timed out: {}", target);
            HookResult {
                action: HookAction::Allow,
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: true,
            }
        }
    };
    if let Some(ref cb) = on_done {
        cb(&handle_id);
    }
    result
}));
```

### WireServer Dispatch

**File:** `src/wire_server/mod.rs`

```rust
// octopus-cli/src/wire_server/mod.rs ~line 441 — WireServer hook dispatch
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

## 7.5 Client-Side Handling

A wire client receives:

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
    "input_data": {
      "hook_event_name": "PreToolUse",
      "session_id": "sess_abc",
      "cwd": "/home/user",
      "tool_name": "shell",
      "tool_input": {"command": "rm -rf /tmp/old"},
      "tool_call_id": "call_xyz"
    }
  }
}
```

The client should:
1. Parse the request.
2. Show a dialog or run its own logic.
3. Respond with a JSON-RPC response:

```json
{
  "jsonrpc": "2.0",
  "id": "uuid-handle",
  "result": {
    "request_id": "uuid-handle",
    "action": "block",
    "reason": "User clicked 'Deny'"
  }
}
```

## 7.6 Server-Side Response Handling

**File:** `src/wire_server/mod.rs`

```rust
// octopus-cli/src/wire_server/mod.rs ~line 586 — WireServer::handle_response
async fn handle_response(&self, resp: JSONRPCClientResponse) {
    let (id, result, error) = match resp {
        JSONRPCClientResponse::Success(s) => (s.id, Some(s.result), None),
        JSONRPCClientResponse::Error(e) => (e.id, None, Some(e.error)),
    };

    let pending = self.pending_requests.lock().await.remove(&id);

    match pending {
        Some(PendingRequest::Hook(handle)) => {
            if error.is_some() {
                handle.resolve(HookAction::Allow);
                return;
            }
            if let Some(result) = result {
                if let Ok(body) = serde_json::from_value::<HookResponse>(result) {
                    let action = if body.action == "block" {
                        HookAction::Block(body.reason)
                    } else {
                        HookAction::Allow
                    };
                    handle.resolve(action);
                } else {
                    tracing::warn!("Invalid hook response for id={}", id);
                    handle.resolve(HookAction::Allow);
                }
            } else {
                handle.resolve(HookAction::Allow);
            }
        }
        // ... Approval handling omitted
    }
}
```

Key points:
- **JSON-RPC error response** → `allow` (fail-open).
- **Validation error** → `allow` (fail-open).
- **Missing fields** → serde defaults `action` to `allow`.

## 7.7 Timeout Handling

Wire-side hooks share the same `timeout` semantics as server-side hooks. The `HookEngine::trigger()` uses `tokio::time::timeout` with the same boundary.

However, the wire protocol itself adds latency:
- Serialization / deserialization.
- Network or stdio overhead.
- Client UI rendering time.

For `PreToolUse`, this means a GUI client might show a modal dialog, and the user has **up to `timeout` seconds** to respond. If they don't, the hook is treated as `allow` (fail-open) and the tool proceeds.

**Python comparison:** The Python version had the same timeout behavior, but the stale `PendingRequest` was never cleaned up from the server's dict. The Rust version adds the `on_wire_hook_done` callback to remove it explicitly.

## 7.8 Wire Events for Observability

The wire protocol also emits events so clients can observe hook execution:

```rust
// octopus-cli/src/soul/kimisoul.rs ~line 260 — Wire events for observability
// When a hook is triggered
WireEvent::HookTriggered(HookTriggered {
    event: event.to_string(),
    target: target.to_string(),
    hook_count: count,
})

// When a hook is resolved
WireEvent::HookResolved(HookResolved {
    event: event.to_string(),
    target: target.to_string(),
    action: action_str,
    reason,
    duration_ms,
})
```

These are fire-and-forget notifications that let a UI show "Waiting for approval..." and then "Approved" or "Blocked".

**Python comparison:** Python emitted the same events but sent them as raw Pydantic models serialized to dicts. Rust uses the `WireEvent` enum:

```rust
// octopus-cli/src/wire/event.rs ~line 325 — WireEvent enum (abbreviated)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireEvent {
    TextPart(TextPart),
    TurnBegin(TurnBegin),
    HookRequest(HookRequest),
    HookResponse(HookResponse),
    HookTriggered(HookTriggered),
    HookResolved(HookResolved),
    // ... 13+ other variants omitted
}
```

This is a **strong enum** (per `AGENTS.md`): the consumer uses an exhaustive `match` instead of trial-and-error deserialization.

## 7.9 Comparison: Server-Side vs. Wire-Side

| Aspect | Server-Side Hook | Wire-Side Hook |
|--------|------------------|----------------|
| **Registration** | `config.toml` | Wire initialization message |
| **Execution** | Local subprocess | Remote client process |
| **Latency** | ~50–100ms (shell spawn) | Variable (UI + network) |
| **Use case** | Scripts, audits, simple filters | GUI dialogs, remote policy servers |
| **Fail-open** | Timeout kills proc → allow | Timeout drops future → allow |
| **Deduplication** | By command string | No deduplication |
| **Security** | Runs as CLI user | Runs in client process |
| **Cleanup** | Process exit cleans up | `on_wire_hook_done` removes pending request |
