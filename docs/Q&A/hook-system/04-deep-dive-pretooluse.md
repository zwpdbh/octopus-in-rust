# 4. Deep Dive: How `PreToolUse` Works

`PreToolUse` is the most critical hook in the system because it is **blocking**: it can prevent a tool from executing entirely. This section traces every line of code from the moment a tool is requested to the moment it either runs or is vetoed.

## 4.1 The Trigger Site

**File:** `src/kimi_cli/soul/toolset.py` (lines 265–291)

When the LLM requests a tool call, `KimiToolset.call()` builds an async closure `_call()` and passes it to a task runner. Inside `_call()`, the very first thing that happens is the `PreToolUse` hook:

```python
async def _call():
    tool_input_dict = arguments if isinstance(arguments, dict) else {}

    # ============================================
    # 1. BUILD THE PAYLOAD
    # ============================================
    from kimi_cli.hooks import events

    results = await self._hook_engine.trigger(
        "PreToolUse",
        matcher_value=tool_call.function.name,   # "shell", "read_file", etc.
        input_data=events.pre_tool_use(
            session_id=_get_session_id(),
            cwd=str(Path.cwd()),
            tool_name=tool_call.function.name,
            tool_input=tool_input_dict,
            tool_call_id=tool_call.id,
        ),
    )

    # ============================================
    # 2. CHECK THE AGGREGATED RESULT
    # ============================================
    for result in results:
        if result.action == "block":
            # Any single "block" vetoes the tool call.
            return ToolResult(
                tool_call_id=tool_call.id,
                return_value=ToolError(
                    message=result.reason or "Blocked by PreToolUse hook",
                    brief="Hook blocked",
                ),
            )

    # ============================================
    # 3. EXECUTE THE TOOL (only if allowed)
    # ============================================
    tool = self._tools[tool_call.function.name]
    result = await tool.run(arguments)
    # ... post-processing
```

This is a **synchronous wait**: the tool execution task is paused until every matched hook handler returns.

## 4.2 Building the Payload

**File:** `src/kimi_cli/hooks/events.py`

```python
def pre_tool_use(
    *,
    session_id: str,
    cwd: str,
    tool_name: str,
    tool_input: dict[str, Any],
    tool_call_id: str = "",
) -> dict[str, Any]:
    return {
        **_base("PreToolUse", session_id, cwd),
        "tool_name": tool_name,
        "tool_input": tool_input,
        "tool_call_id": tool_call_id,
    }
```

For a `shell` tool call with `{"command": "rm -rf /tmp/old"}`, the JSON sent to stdin looks like:

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

## 4.3 The Engine Trigger Method

**File:** `src/kimi_cli/hooks/engine.py`

```python
async def trigger(
    self,
    event: HookEventType,
    *,
    matcher_value: str = "",
    input_data: dict[str, Any],
) -> list[HookResult]:
    """
    1. Find all server-side hooks matching event + regex.
    2. Find all wire subscriptions matching event + regex.
    3. Deduplicate server hooks by command string.
    4. Run everything in parallel.
    5. Aggregate: block wins.
    6. Emit telemetry.
    """
```

### Step-by-step inside `trigger()`:

#### Step 1: Match server-side hooks

```python
server_hooks = self._by_event.get(event, [])
matched = []
for h in server_hooks:
    if not h.matcher or re.search(h.matcher, matcher_value):
        matched.append(h)
```

Example: If `matcher_value` is `"shell"` and a hook has `matcher="shell|bash"`, it matches.

#### Step 2: Deduplicate

```python
seen_commands = set()
deduped = []
for h in matched:
    if h.command not in seen_commands:
        seen_commands.add(h.command)
        deduped.append(h)
```

This prevents the same shell script from running twice if it was registered twice.

#### Step 3: Match wire subscriptions

```python
wire_subs = self._wire_by_event.get(event, [])
matched_wire = []
for sub in wire_subs:
    if not sub.matcher or re.search(sub.matcher, matcher_value):
        matched_wire.append(sub)
```

#### Step 4: Create futures and run in parallel

```python
futures: list[asyncio.Future[HookResult]] = []

# Server-side: spawn subprocesses
for h in deduped:
    futures.append(
        asyncio.create_task(
            run_hook(h.command, input_data, timeout=h.timeout, cwd=self._cwd)
        )
    )

# Wire-side: create handles and dispatch
for sub in matched_wire:
    handle = WireHookHandle(
        id=str(uuid.uuid4()),
        subscription_id=sub.id,
        event=event,
        target=matcher_value,
        input_data=input_data,
        _future=asyncio.get_event_loop().create_future(),
    )
    self._pending_wire_hooks[handle.id] = handle
    self._on_wire_hook(handle)  # dispatches to wire server
    futures.append(handle._future)

# Wait for all
results = await asyncio.gather(*futures, return_exceptions=True)
```

#### Step 5: Aggregate with fail-open

```python
parsed_results: list[HookResult] = []
for r in results:
    if isinstance(r, Exception):
        # Timeout, subprocess crash, etc. → default allow
        parsed_results.append(HookResult(action="allow", reason=str(r)))
    else:
        parsed_results.append(r)

# Telemetry is emitted here (outside the try/except so crashes don't affect logic)
```

## 4.4 The Server-Side Runner

**File:** `src/kimi_cli/hooks/runner.py`

For each matched `HookDef`, the engine calls:

```python
async def run_hook(command: str, input_data: dict[str, Any], *, timeout: int = 30, cwd: str | None = None) -> HookResult:
    proc = await asyncio.create_subprocess_shell(
        command,
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        cwd=cwd,
    )

    stdout, stderr = await asyncio.wait_for(
        proc.communicate(input=json.dumps(input_data).encode()),
        timeout=timeout,
    )

    # === DECISION LOGIC ===

    # Exit code 2 → explicit block (reason from stderr)
    if proc.returncode == 2:
        return HookResult(action="block", reason=stderr.decode().strip() or "Blocked")

    # Exit code 0 + JSON stdout with deny → block
    if proc.returncode == 0:
        try:
            parsed = json.loads(stdout.decode())
            if parsed.get("hookSpecificOutput", {}).get("permissionDecision") == "deny":
                return HookResult(action="block", reason="Denied by hook output")
        except (json.JSONDecodeError, AttributeError):
            pass
        return HookResult(action="allow")

    # Any other exit code → allow (fail-open)
    return HookResult(action="allow")
```

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

## 4.5 The Wire-Side Path

If a wire client has subscribed to `PreToolUse`, the engine also creates a `WireHookHandle`.

**File:** `src/kimi_cli/wire/server.py` (lines 481–499)

```python
async def _on_wire_hook(handle: WireHookHandle) -> None:
    request = HookRequest(
        id=handle.id,
        subscription_id=handle.subscription_id,
        event=handle.event,
        target=handle.target,
        input_data=handle.input_data,
    )
    self._pending_requests[handle.id] = request
    await self._send_msg(JSONRPCRequestMessage(id=handle.id, params=request))
    action, reason = await request.wait()
    handle.resolve(action, reason)
```

The client receives:

```json
{
  "jsonrpc": "2.0",
  "id": "handle_uuid",
  "method": "HookRequest",
  "params": {
    "id": "handle_uuid",
    "subscription_id": "sub_abc",
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
  "id": "handle_uuid",
  "result": {
    "request_id": "handle_uuid",
    "action": "block",
    "reason": "User denied this action"
  }
}
```

## 4.6 Back to the Tool: Block or Execute?

**File:** `src/kimi_cli/soul/toolset.py`

```python
for result in results:
    if result.action == "block":
        return ToolResult(
            tool_call_id=tool_call.id,
            return_value=ToolError(
                message=result.reason or "Blocked by PreToolUse hook",
                brief="Hook blocked",
            ),
        )
```

If **no** hook blocked, execution continues to the actual tool:

```python
tool = self._tools[tool_call.function.name]
result = await tool.run(arguments)
```

## 4.7 Complete Sequence Diagram

```
LLM / Planner          KimiToolset.call()          HookEngine          run_hook()          WireServer          Client
    │                        │                         │                   │                  │                │
    │── "call shell" ───────▶│                         │                   │                  │                │
    │                        │                         │                   │                  │                │
    │                        │── build payload ───────▶│                   │                  │                │
    │                        │   (events.pre_tool_use) │                   │                  │                │
    │                        │                         │                   │                  │                │
    │                        │── engine.trigger() ────▶│                   │                  │                │
    │                        │                         │                   │                  │                │
    │                        │                         │── match + dedup ──┤                  │                │
    │                        │                         │                   │                  │                │
    │                        │                         │── run_hook() ────▶│                  │                │
    │                        │                         │   (server hook)   │── subprocess ────┤                │
    │                        │                         │                   │   (JSON stdin)   │                │
    │                        │                         │                   │                  │                │
    │                        │                         │                   │◀─ exit 2 ────────┤                │
    │                        │                         │◀─ HookResult ────│                  │                │
    │                        │                         │   (block)         │                  │                │
    │                        │                         │                   │                  │                │
    │                        │                         │── wire handle ───────────────────────▶│── request ────▶│
    │                        │                         │                   │                  │                │
    │                        │                         │◀──────────────────────────────────────│◀─ response ───│
    │                        │                         │                   │                  │                │
    │                        │◀─ [HookResult(block)] ──│                   │                  │                │
    │                        │                         │                   │                  │                │
    │                        │── check results ────────┤                   │                  │                │
    │                        │   action == "block"     │                   │                  │                │
    │                        │                         │                   │                  │                │
    │◀── ToolError ──────────│                       │                   │                  │                │
    │   "Blocked by hook"    │                       │                   │                  │                │
```

## 4.8 Key Invariants

1. **Block wins over allow**: Even if 9 hooks say `allow` and 1 says `block`, the tool is blocked.
2. **Fail-open**: A crashed hook, timeout, or malformed response is treated as `allow`.
3. **Parallel, not serial**: All matched hooks run simultaneously; total latency is the slowest hook, not the sum.
4. **Regex filtering**: A hook only runs if its `matcher` regex matches the `matcher_value` (tool name for `PreToolUse`).
