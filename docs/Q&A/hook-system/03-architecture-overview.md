# 3. Architecture Overview

The hook system in `tmp/kimi-cli` is a **hybrid local + remote permission layer**. It is implemented across five Python modules and interacts with both the local file system and remote wire clients.

## 3.1 Module Map

```
kimi_cli/hooks/
├── __init__.py          # Re-exports
├── config.py            # HookDef, HookEventType, TOML parsing
├── engine.py            # HookEngine — matching, dispatch, aggregation
├── events.py            # Payload builders (pre_tool_use, post_tool_use, ...)
└── runner.py            # run_hook() — subprocess execution

kimi_cli/soul/
├── toolset.py           # Triggers PreToolUse / PostToolUse / PostToolUseFailure
└── kimisoul.py          # Triggers SessionStart, Stop, UserPromptSubmit, etc.

kimi_cli/wire/
├── server.py            # Dispatches wire-side hooks via JSON-RPC
└── types.py             # HookRequest, HookResponse, WireHookSubscription
```

## 3.2 Core Types

### HookDef — The Configuration

**File:** `src/kimi_cli/hooks/config.py`

```python
HookEventType = Literal[
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "UserPromptSubmit",
    "Stop",
    "StopFailure",
    "SessionStart",
    "SessionEnd",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "Notification",
]

class HookDef(BaseModel):
    event: HookEventType      # Which lifecycle moment to attach to
    command: str              # Shell command to run (server-side)
    matcher: str = ""         # Regex filter; "" matches everything
    timeout: int = Field(default=30, ge=1, le=600)  # Seconds
```

Hooks are loaded from `config.toml`:

```toml
[[hooks]]
event = "PreToolUse"
command = "python /home/user/hooks/block_shell.py"
matcher = "shell"
timeout = 5

[[hooks]]
event = "SessionStart"
command = "echo 'New session started' >> /tmp/sessions.log"
```

### HookResult — The Decision

**File:** `src/kimi_cli/hooks/engine.py` (conceptual)

```python
@dataclass
class HookResult:
    action: Literal["allow", "block"] = "allow"
    reason: str = ""
```

### WireHookSubscription — Remote Interest

**File:** `src/kimi_cli/hooks/engine.py`

```python
@dataclass
class WireHookSubscription:
    id: str
    event: str
    matcher: str = ""
    timeout: int = 30
```

When a client connects via the wire protocol, it can subscribe to hooks remotely. The server then forwards matching events to the client and awaits a decision.

## 3.3 The HookEngine

**File:** `src/kimi_cli/hooks/engine.py`

The `HookEngine` is the central dispatcher. It maintains two indexes:

```python
class HookEngine:
    def __init__(self, ...):
        self._hooks: list[HookDef] = []           # server-side hooks from config
        self._wire_subs: list[WireHookSubscription] = []  # remote subscriptions
        self._by_event: dict[str, list[HookDef]] = {}
        self._wire_by_event: dict[str, list[WireHookSubscription]] = {}
```

### Registration

```python
engine.add_hooks([HookDef(event="PreToolUse", command="...", matcher="shell")])
engine.add_wire_subscriptions([WireHookSubscription(id="abc", event="PreToolUse")])
```

### Trigger Flow

```
Core calls engine.trigger("PreToolUse", matcher_value="shell", input_data={...})
                    │
                    ▼
        ┌───────────────────────┐
        │ 1. Match by event     │ ──▶ Find all HookDefs with event="PreToolUse"
        │ 2. Filter by regex    │ ──▶ Keep those where matcher matches "shell"
        │ 3. Deduplicate        │ ──▶ Skip duplicate commands
        │ 4. Match wire subs    │ ──▶ Same for WireHookSubscriptions
        │ 5. Run in parallel    │ ──▶ asyncio.gather(server_hooks + wire_hooks)
        │ 6. Aggregate          │ ──▶ block if ANY result.action == "block"
        │ 7. Telemetry          │ ──▶ Log timing (outside fail-open guard)
        └───────────────────────┘
```

## 3.4 Payload Builders

**File:** `src/kimi_cli/hooks/events.py`

Instead of an enum, payloads are plain `dict[str, Any]` built by helper functions:

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

This is the pattern for **all** events. The `_base()` helper injects common fields:

```python
def _base(event_name: str, session_id: str, cwd: str) -> dict[str, Any]:
    return {
        "hook_event_name": event_name,
        "session_id": session_id,
        "cwd": cwd,
    }
```

## 3.5 Server-Side Runner

**File:** `src/kimi_cli/hooks/runner.py`

```python
async def run_hook(command: str, input_data: dict[str, Any], *, timeout: int = 30, cwd: str | None = None) -> HookResult:
    proc = await asyncio.create_subprocess_shell(
        command, stdin=PIPE, stdout=PIPE, stderr=PIPE, cwd=cwd
    )
    stdout, stderr = await asyncio.wait_for(
        proc.communicate(input=json.dumps(input_data).encode()),
        timeout=timeout,
    )
    # Decision logic based on exit code and stdout
```

The runner:
1. Spawns the configured `command` as a shell subprocess.
2. Writes the JSON payload to **stdin**.
3. Waits for **stdout**, **stderr**, and **exit code**.
4. Interprets the result (see section 6).

## 3.6 Wire-Side Flow

**Files:** `src/kimi_cli/wire/server.py`, `src/kimi_cli/wire/types.py`

When a wire subscription matches:

```
HookEngine                       WireServer                      Client
    │                               │                              │
    │──▶ create WireHookHandle ────▶│                              │
    │                               │──▶ send JSON-RPC request ───▶│
    │                               │    (HookRequest)             │
    │                               │                              │
    │                               │◀── send JSON-RPC response ──│
    │                               │    (HookResponse)            │
    │◀── resolve handle ────────────│                              │
```

The engine awaits the future inside `WireHookHandle` just like it awaits a local subprocess.

## 3.7 Lifecycle Diagram

```
┌─────────────┐
│ SessionStart│◀──── fire-and-forget
└──────┬──────┘
       │
       ▼
┌─────────────┐     ┌─────────────────┐
│UserPrompt   │◀────│ can block turn  │
│Submit       │     │ (e.g. profanity)│
└──────┬──────┘     └─────────────────┘
       │
       ▼
┌─────────────┐     ┌─────────────────────────────┐
│  Tool Call  │────▶│ PreToolUse (blocking)       │
│  Request    │     │ ──▶ shell / wire handler    │
└──────┬──────┘     │ ──▶ if block: return error  │
       │            └─────────────────────────────┘
       ▼
┌─────────────┐
│ Tool Exec   │
└──────┬──────┘
       │
   ┌───┴───┐
   │       │
   ▼       ▼
┌──────┐ ┌──────────┐
│Success│ │ Failure  │
└──┬───┘ └────┬─────┘
   │          │
   ▼          ▼
┌────────┐ ┌─────────────┐
│PostTool│ │PostToolUse  │◀──── fire-and-forget
│Use     │ │Failure      │
└────────┘ └─────────────┘
       │
       ▼
┌─────────────┐
│     Stop    │◀──── can inject follow-up prompt
└──────┬──────┘
       │
       ▼
┌─────────────┐
│ SessionEnd  │◀──── fire-and-forget
└─────────────┘
```
