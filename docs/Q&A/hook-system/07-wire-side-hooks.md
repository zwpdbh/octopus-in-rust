# 7. Wire-Side Hooks

Wire-side hooks extend the hook system across process boundaries. When a client connects to the CLI via the wire protocol (JSON-RPC over stdio or socket), it can subscribe to events. The server then forwards matching events to the client and waits for a decision.

## 7.1 Why Wire-Side Hooks Exist

Not all hook logic can live in a local shell script:

- **GUI clients** want to show a confirmation dialog before a dangerous tool runs.
- **Remote agents** need to enforce policies from a central server.
- **IDE plugins** want to intercept file writes to update their own state.

Wire-side hooks let these clients participate in the hook system as first-class citizens.

## 7.2 Types

**File:** `src/kimi_cli/wire/types.py`

### WireHookSubscription

```python
@dataclass
class WireHookSubscription:
    id: str           # Unique subscription ID from client
    event: str        # "PreToolUse", "Stop", etc.
    matcher: str = "" # Regex filter
    timeout: int = 30 # How long the server waits
```

Sent by the client during wire initialization.

### HookRequest

```python
class HookRequest(BaseModel):
    id: str
    subscription_id: str = ""
    event: str
    target: str = ""          # matcher_value (e.g., tool name)
    input_data: dict[str, Any] = Field(default_factory=dict)

    _resolved: asyncio.Event = PrivateAttr(default_factory=asyncio.Event)
    _action: str = PrivateAttr(default="allow")
    _reason: str = PrivateAttr(default="")

    async def wait(self) -> tuple[Literal["allow", "block"], str]:
        await self._resolved.wait()
        return self._action, self._reason

    def resolve(self, action: str, reason: str = "") -> None:
        self._action = action
        self._reason = reason
        self._resolved.set()
```

### HookResponse

```python
class HookResponse(BaseModel):
    request_id: str
    action: Literal["allow", "block"] = "allow"
    reason: str = ""
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

## 7.4 Trigger Flow

When `HookEngine.trigger()` matches a wire subscription:

```python
handle = WireHookHandle(
    id=str(uuid.uuid4()),
    subscription_id=sub.id,
    event=event,
    target=matcher_value,
    input_data=input_data,
    _future=asyncio.get_event_loop().create_future(),
)
self._pending_wire_hooks[handle.id] = handle
asyncio.create_task(self._on_wire_hook(handle))
```

### WireServer Dispatch

**File:** `src/kimi_cli/wire/server.py` (lines 481–499)

```python
async def _on_wire_hook(self, handle: WireHookHandle) -> None:
    request = HookRequest(
        id=handle.id,
        subscription_id=handle.subscription_id,
        event=handle.event,
        target=handle.target,
        input_data=handle.input_data,
    )
    self._pending_requests[handle.id] = request

    # Send to client
    await self._send_msg(JSONRPCRequestMessage(
        id=handle.id,
        method="HookRequest",
        params=request.model_dump(),
    ))

    # Wait for client response
    action, reason = await request.wait()
    handle.resolve(action, reason)
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

Or, to allow:

```json
{
  "jsonrpc": "2.0",
  "id": "uuid-handle",
  "result": {
    "request_id": "uuid-handle",
    "action": "allow",
    "reason": ""
  }
}
```

## 7.6 Server-Side Response Handling

**File:** `src/kimi_cli/wire/server.py` (lines 995–1017)

```python
case HookRequest():
    if isinstance(msg, JSONRPCErrorResponse):
        # Client sent an error → fail-open
        request.resolve("allow")
        return

    try:
        result = HookResponse.model_validate(msg.result)
    except pydantic.ValidationError:
        # Malformed response → fail-open
        request.resolve("allow")
        return

    request.resolve(result.action, result.reason)
```

Key points:
- **JSON-RPC error response** → `allow`.
- **Validation error** → `allow`.
- **Missing fields** → Pydantic defaults action to `allow`.

## 7.7 Timeout Handling

Wire-side hooks share the same `timeout` semantics as server-side hooks. The `HookEngine.trigger()` uses `asyncio.gather()` with the same timeout boundary.

However, the wire protocol itself adds latency:
- Serialization / deserialization.
- Network or stdio overhead.
- Client UI rendering time.

For `PreToolUse`, this means a GUI client might show a modal dialog, and the user has **up to `timeout` seconds** to respond. If they don't, the hook is treated as `allow` (fail-open) and the tool proceeds.

## 7.8 Wire Events for Observability

The wire protocol also emits events so clients can observe hook execution:

```python
# When a hook is triggered
WireEvent::HookTriggered(HookTriggered {
    request_id: handle.id,
    subscription_id: sub.id,
    event: event,
    target: matcher_value,
})

# When a hook is resolved
WireEvent::HookResolved(HookResolved {
    request_id: handle.id,
    action: action,
    reason: reason,
})
```

These are fire-and-forget notifications that let a UI show "Waiting for approval..." and then "Approved" or "Blocked".

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
