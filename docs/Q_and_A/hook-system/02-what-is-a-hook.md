# 2. What Is a Hook?

> *"A hook is an extension point that lets external code intercept, observe, or modify behavior at specific moments in the execution flow—without changing the core system's source code."*

## 2.1 The Problem Hooks Solve

Imagine a CLI tool that can call arbitrary tools (file system, shell, network). You want to:

- **Block** dangerous shell commands in certain directories.
- **Log** every file write for audit purposes.
- **Inject** a confirmation prompt before deleting data.
- **Notify** an external service when a session starts.

You *could* hardcode all of this into the core tool logic, but that makes the system:
- **Rigid** — every new policy requires a code change and redeploy.
- **Bloated** — the core accumulates unrelated concerns.
- **Hard to test** — you must mock the entire system to test one policy.

A **hook system** solves this by saying:

> *"I will pause at specific moments and ask anyone who cares: 'Should I proceed? Here's the context.'"*

## 2.2 Anatomy of a Hook

Every hook system has four parts:

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  1. Event       │────▶│  2. Payload     │────▶│  3. Handler     │────▶│  4. Decision    │
│  (when?)        │     │  (what data?)   │     │  (who responds?)│     │  (what next?)   │
└─────────────────┘     └─────────────────┘     └─────────────────┘     └─────────────────┘
```

| Part | Description | Example (`PreToolUse`) |
|------|-------------|------------------------|
| **Event** | A named moment in the lifecycle. | Before a tool executes. |
| **Payload** | Structured data about the current state. | `{tool_name: "shell", tool_input: {command: "rm -rf /"}, cwd: "/home/user", ...}` |
| **Handler** | External code that receives the payload. | A shell script, a Rust function, or a remote client over JSON-RPC. |
| **Decision** | What the core does with the handler's response. | If any handler says `block`, abort the tool call and return an error. |

## 2.3 Hook Patterns

### Blocking / Permission Hook
The handler can veto an action. Used for **safety** and **policy enforcement**.

```
Core: "I'm about to run `shell` with `rm -rf /`."
Hook: "BLOCK — that command is not allowed here."
Core: Abort tool call, return ToolError.
```

`PreToolUse` is a **blocking hook**.

### Fire-and-Forget / Observer Hook
The handler observes but cannot change the outcome. Used for **logging**, **metrics**, **notifications**.

```
Core: "I just ran `shell` and it failed."
Hook: (logs the failure to a file)
Core: Continues normally.
```

`PostToolUse`, `PostToolUseFailure` are **observer hooks**.

### Transform Hook
The handler modifies the payload before the core proceeds. Used for **input sanitization**, **enrichment**.

The current octopus-cli hook system does **not** use transform hooks; it only supports `allow` / `block` decisions.

## 2.4 Where Hooks Live in the Architecture

```
┌─────────────────────────────────────────────┐
│              User / Client                  │
└─────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────┐
│           CLI / Soul (Core)                 │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐     │
│  │ Session │  │  Turn   │  │  Tool   │     │
│  │ Start   │  │  Loop   │  │  Call   │     │
│  └────┬────┘  └────┬────┘  └────┬────┘     │
│       │            │            │           │
│       ▼            ▼            ▼           │
│  ┌─────────────────────────────────────┐    │
│  │         HookEngine                  │    │
│  │  ┌─────────┐      ┌─────────────┐  │    │
│  │  │ Server  │      │   Wire      │  │    │
│  │  │ Hooks   │      │ Subscriptions│  │    │
│  │  └─────────┘      └─────────────┘  │    │
│  └─────────────────────────────────────┘    │
└─────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────┐
│    External Handlers (Scripts / Clients)    │
└─────────────────────────────────────────────┘
```

The **HookEngine** sits between the core business logic and external handlers. It is:
- **Synchronous** for blocking hooks (the core waits for a decision).
- **Asynchronous / fire-and-forget** for observer hooks (the core does not wait).

## 2.5 Key Design Decisions in octopus-cli

1. **Fail-open**: If a hook crashes, times out, or returns invalid data, the default is `allow`. Safety hooks should be tested carefully.
2. **Parallel execution**: All matched hooks run concurrently; the result is aggregated (`block` wins).
3. **Compiled regex matching**: Each hook declares a `matcher` regex. We compile it **once** at load time using `regex::Regex` (not on every trigger like the Python original did).
4. **Two backends**: Server-side (local `tokio::process::Command`) and wire-side (remote client over JSON-RPC).
5. **Strong typing**: `HookEvent` is an enum with associated data, not a string literal. Payloads are typed variants, not `dict[str, Any]` or `serde_json::Value`.

## 2.7 Concrete Example: A Workspace-Safety Hook

Here is what a real server-side hook looks like from the user's perspective.

### `config.toml`

```toml
# ~/.config/octopus/config.toml
[[hooks]]
event = "PreToolUse"
matcher = "Shell|WriteFile"
command = "python /home/user/.config/octopus/hooks/block_outside_workspace.py"
timeout = 5
```

This tells the engine:

- **When**: before any `Shell` or `WriteFile` tool runs (`event = "PreToolUse"`).
- **Which tools**: only those whose name matches the regex `Shell|WriteFile` (`matcher`).
- **What to run**: the Python script (`command`).
- **How long to wait**: 5 seconds (`timeout`).

### The Hook Script

```python
# /home/user/.config/octopus/hooks/block_outside_workspace.py
import json
import sys

payload = json.load(sys.stdin)
cwd = payload.get("cwd", "")
tool_name = payload.get("tool_name", "")

if not cwd.startswith("/home/user/work"):
    print(
        f"Refusing {tool_name} outside workspace: {cwd}",
        file=sys.stderr,
    )
    sys.exit(2)  # exit 2 = block

sys.exit(0)  # exit 0 = allow
```

When the agent tries to run `Shell` or `WriteFile` outside `/home/user/work`, the hook receives JSON like this on stdin:

```json
{
  "hook_event_name": "PreToolUse",
  "session_id": "sess_abc123",
  "cwd": "/etc",
  "tool_name": "WriteFile",
  "tool_input": { "path": "/etc/passwd", "content": "..." },
  "tool_call_id": "call_xyz789"
}
```

Because `cwd` is `/etc`, the script exits with code `2`. The engine treats that as a **block**, and the tool never runs.

**Key takeaways:**
- The user config never mentions `session_id`, `cwd`, or `tool_input` — those are runtime payload fields.
- The config only declares the **event type** (`PreToolUse`) and the **filter regex** (`matcher`).
- The hook script is a normal program that reads JSON from stdin and decides via exit code.

## 2.8 Python vs. Rust at a Glance

| Aspect | Python (kimi-cli) | Rust (octopus-cli) |
|--------|-------------------|-------------------|
| **Event type** | `Literal["PreToolUse", ...]` | `enum HookEvent` with discriminant-only `Eq`/`Hash` |
| **Payload** | `dict[str, Any]` built by helpers | `enum HookPayload` (variant associated data) |
| **Subprocess** | `asyncio.create_subprocess_shell` | `tokio::process::Command` |
| **Regex** | `re.search(pattern, value)` on every trigger | `Regex::new` once at load; `re.is_match` at trigger |
| **Wire carrier** | Pydantic models sent as plain dicts | `enum WireEvent` with `#[serde(untagged)]` |
| **Clone cost** | Dicts passed by reference (but task spawn copies) | `Arc<HookEvent>` shared across tasks; JSON serialized once |
| **Stdout parsing** | Manual `dict.get("hookSpecificOutput")` | Typed `HookStdout` struct with `Deserialize` |
