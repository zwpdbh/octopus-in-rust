# 3. Architecture Overview

The hook system in `octopus-cli` is a **hybrid local + remote permission layer**. It is implemented across six Rust modules and interacts with both the local file system and remote wire clients.

## 3.1 Module Map

```
octopus-cli/src/
├── hooks/
│   ├── mod.rs          # Re-exports
│   ├── event.rs        # HookEvent enum + discriminant serde helpers
│   ├── engine.rs       # HookEngine — matching, dispatch, aggregation
│   └── runner.rs       # run_hook() — subprocess execution + typed stdout parsing
├── config.rs           # HookDef struct for config deserialization
├── wire/
│   └── event.rs        # Wire events: HookTriggered, HookResolved, HookRequest
├── wire/
│   └── jsonrpc.rs      # HookResponse (client → server)
└── wire_server/
    └── mod.rs          # Wire-server integration: subscriptions, dispatch, responses
```

## 3.2 Core Types

### HookEvent — The Strong Enum

**File:** `src/hooks/event.rs`

```rust
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
    PostToolUse { ... },
    PostToolUseFailure { ... },
    UserPromptSubmit { ... },
    Stop { ... },
    // ... every event type
}
```

**Key design:** equality and hashing are **discriminant-only**:

```rust
impl PartialEq for HookEvent {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
```

This means `HookEvent::PreToolUse { tool_name: "A", ... }` == `HookEvent::PreToolUse { tool_name: "B", ... }`. The same type acts as both a **payload** and a **HashMap key** in the engine.

**Python comparison:** The Python version used `HookEventType = Literal["PreToolUse", ...]` — a string literal. Typos were runtime errors; refactoring required `grep`. In Rust, `HookEvent::PreToolUse` is checked at compile time.

### HookDef — The Configuration

**File:** `src/config.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    #[serde(with = "crate::hooks::event::discriminant_serde")]
    pub event: crate::hooks::HookEvent,
    pub matcher: Option<String>,
    #[serde(skip)]
    pub compiled_matcher: Option<Regex>,
    pub command: String,
    pub timeout: u64,
}
```

Hooks are loaded from `config.toml` under `[[hooks]]` tables:

```toml
[[hooks]]
event = "PreToolUse"
command = "python /home/user/hooks/block_shell.py"
matcher = "shell"
timeout = 5
```

The `discriminant_serde` helper deserializes `"PreToolUse"` into `HookEvent::PreToolUse` with empty default fields. The `compiled_matcher` field is populated once at load time by `HookDef::compile_matcher()`.

**Python comparison:** Python's `HookDef` had no compiled regex field; `re.search` was called on every trigger.

### HookResult — The Decision

**File:** `src/hooks/runner.rs`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action", content = "reason")]
pub enum HookAction {
    Allow,
    Block(String),
}

pub struct HookResult {
    pub action: HookAction,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}
```

### WireHookSubscription — Remote Interest

**File:** `src/hooks/engine.rs`

```rust
#[derive(Debug, Clone)]
pub struct WireHookSubscription {
    pub id: String,
    pub event: HookEvent,
    pub matcher: String,
    pub compiled_matcher: Option<Regex>,
    pub timeout: u64,
}
```

When a client connects via the wire protocol, it can subscribe to hooks remotely. The server then forwards matching events to the client and awaits a decision.

## 3.3 The HookEngine

**File:** `src/hooks/engine.rs`

The `HookEngine` is the central dispatcher. It maintains two indexes:

```rust
pub struct HookEngine {
    hooks: Vec<HookDef>,
    wire_subs: Vec<WireHookSubscription>,
    by_event: HashMap<HookEvent, Vec<HookDef>>,
    wire_by_event: HashMap<HookEvent, Vec<WireHookSubscription>>,
    // callbacks omitted
}
```

### Registration

```rust
engine.add_hooks(vec![hook_def]);        // server-side from config
engine.add_wire_subscriptions(vec![sub]); // remote subscriptions
```

Both methods call `rebuild_index()`, which groups hooks by discriminant for O(1) lookup.

### Trigger Flow

```
Core calls engine.trigger(HookEvent::PreToolUse { ... }, "shell")
                    │
                    ▼
        ┌───────────────────────┐
        │ 1. Wrap in Arc        │ ──▶ Avoid cloning payload per hook
        │ 2. Pre-serialize JSON │ ──▶ Avoid re-serializing per wire hook
        │ 3. Match by discriminant│ ──▶ O(1) HashMap lookup
        │ 4. Filter by regex    │ ──▶ Use pre-compiled Regex::is_match
        │ 5. Deduplicate        │ ──▶ Skip duplicate commands
        │ 6. Match wire subs    │ ──▶ Same logic for remote hooks
        │ 7. Run in parallel    │ ──▶ tokio::spawn + try_join_all
        │ 8. Aggregate          │ ──▶ block if ANY result.action == Block
        │ 9. Cleanup callbacks  │ ──▶ on_wire_hook_done removes stale handles
        └───────────────────────┘
```

## 3.4 Payload Construction

In Rust, payloads are constructed by calling typed constructors on `HookEvent`:

```rust
let event = HookEvent::pre_tool_use(
    session_id,
    cwd,
    tool_name,
    &tool_input,
    tool_call_id,
);
```

This returns a fully typed enum variant. Serialization to JSON happens only when needed (e.g., before writing to stdin or sending over the wire).

**Python comparison:** The Python version used helper functions that built `dict[str, Any]`:

```python
def pre_tool_use(session_id, cwd, tool_name, ...):
    return {
        **_base("PreToolUse", session_id, cwd),
        "tool_name": tool_name,
        "tool_input": tool_input,
    }
```

The Rust approach eliminates typos in field names and guarantees every construction site provides all required fields.

## 3.5 Server-Side Runner

**File:** `src/hooks/runner.rs`

```rust
pub async fn run_hook(
    command: &str,
    event: &HookEvent,
    timeout_secs: u64,
    cwd: Option<&std::path::Path>,
) -> HookResult {
    let json_input = serde_json::to_vec(event)?;
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // ... write JSON to stdin, read stdout/stderr, interpret result
}
```

**Python comparison:** Python used `asyncio.create_subprocess_shell` with the same `sh -c` pattern. The protocol (JSON on stdin, exit codes + stdout for decision) is identical for backward compatibility.

## 3.6 Wire-Side Flow

When a wire subscription matches, the engine creates a `WireHookHandle`:

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

**Python comparison:** The Python wire protocol used trial-and-error deserialization:

```python
if let Ok(req) = serde_json::from_value::<ApprovalRequestEvent>(value.clone()):
    ...
elif let Ok(text) = serde_json::from_value::<TextPart>(value):
    ...
```

Rust uses a single `enum WireEvent` with `#[serde(untagged)]`, so deserialization is type-safe and exhaustive.

## 3.7 Lifecycle Diagram

```
┌─────────────┐
│UserPrompt   │◀──── can block turn
│Submit       │
└──────┬──────┘
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
└─────────────┘
```
