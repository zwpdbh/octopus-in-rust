# 3. Architecture Overview

The hook system in `octopus-cli` is a **hybrid local + remote permission layer**. It is implemented across seven Rust modules and interacts with both the local file system and remote wire clients.

## 3.1 Module Map

```
octopus-cli/src/
├── hooks/
│   ├── mod.rs          # Re-exports
│   ├── event.rs        # HookEventKind + HookEvent enums
│   ├── hook.rs         # Hook trait + CommandHook + WireHook + contexts
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

### HookEventKind — The Registry Key

**File:** `src/hooks/event.rs`

```rust
// octopus-cli/src/hooks/event.rs ~line 12 — HookEventKind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEventKind {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    UserPromptSubmit,
    Stop,
    StopFailure,
    PreCompact,
    PostCompact,
    Notification,
}
```

`HookEventKind` is a **discriminant-only** enum. It carries no runtime data, implements `Hash`/`Eq`, and serializes to just the PascalCase variant name (e.g., `"PreToolUse"`). It is used for:

- `config.toml` keys (`event = "PreToolUse"`).
- The `HookEngine` registry index.
- Wire subscription event names.

### HookEvent — The Runtime Payload

**File:** `src/hooks/event.rs`

```rust
// octopus-cli/src/hooks/event.rs ~line 31 — HookEvent (runtime payload)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

`HookEvent` is the **concrete runtime payload**. It always carries the full data available when the event fires and is serialized to the full JSON payload when sent to hook scripts or wire clients.

The split avoids the awkward dual role where one type tried to be both a config key and a payload:

```rust
// octopus-cli/src/hooks/event.rs ~line 286 — kind + matcher_value helpers
impl HookEvent {
    pub fn kind(&self) -> HookEventKind { ... }
    pub fn matcher_value(&self) -> Option<&str> { ... }
}
```

**Python comparison:** The Python version used `HookEventType = Literal["PreToolUse", ...]` — a string literal. Typos were runtime errors; refactoring required `grep`. In Rust, `HookEventKind::PreToolUse` is checked at compile time, while `HookEvent::PreToolUse { ... }` still carries the full typed payload.

### HookDef — The Configuration

**File:** `src/config.rs`

```rust
// octopus-cli/src/config.rs ~line 1 — HookDef
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    pub event: crate::hooks::HookEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    #[serde(skip)]
    pub compiled_matcher: Option<Regex>,
    pub command: String,
    pub timeout: u64,
}
```

Hooks are loaded from `config.toml` under `[[hooks]]` tables:

```toml
# ~/.config/octopus/config.toml
[[hooks]]
event = "PreToolUse"
matcher = "Shell|WriteFile"
command = "python /home/user/.config/octopus/hooks/block_shell.py"
timeout = 5
```

| Field | Required | Meaning |
|-------|----------|---------|
| `event` | Yes | Which lifecycle moment to intercept (e.g., `PreToolUse`, `UserPromptSubmit`). This selects the `HookEventKind`. |
| `matcher` | No | A regex. The hook only runs when this regex matches the event's natural matcher field (`tool_name` for `PreToolUse`, `prompt` for `UserPromptSubmit`). Empty or omitted matches everything. |
| `command` | Yes | The shell command to execute. It receives the full `HookEvent` JSON on stdin. |
| `timeout` | No | Seconds to wait before treating the hook as failed (default 30). |

The `event` field deserializes directly into `HookEventKind` thanks to `#[serde(rename_all = "PascalCase")]`. The runtime payload (`session_id`, `cwd`, `tool_name`, etc.) is filled in later when the event is triggered. The `compiled_matcher` field is populated once at load time by `HookDef::compile_matcher()`.

**Python comparison:** Python's `HookDef` had no compiled regex field; `re.search` was called on every trigger.

### HookResult — The Decision

**File:** `src/hooks/runner.rs`

```rust
// octopus-cli/src/hooks/runner.rs ~line 21 — HookAction + HookResult
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action", content = "reason")]
pub enum HookAction {
    Allow,
    Block(String),
}

#[derive(Debug, Clone)]
pub struct HookResult {
    pub action: HookAction,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
}
```

### Hook — The Unified Runtime Interface

**File:** `src/hooks/hook.rs`

Server-side and wire-side hooks are exposed through the same trait:

```rust
// octopus-cli/src/hooks/hook.rs ~line 65 — Hook trait
#[async_trait::async_trait]
pub trait Hook: Send + Sync + std::fmt::Debug + HookClone {
    fn kind(&self) -> HookEventKind;
    fn matcher(&self) -> Option<&Regex>;
    fn source(&self) -> &'static str;
    async fn run(&self, event: &HookEvent, ctx: &HookRunContext) -> HookResult;
}
```

Two implementations exist:

- `CommandHook` — runs a local shell command via `run_hook()`.
- `WireHook` — forwards the event to a wire client and awaits a decision.

This lightweight factory pattern lets `HookEngine` treat both sources uniformly: it only needs `kind()` for indexing, `matcher()` for filtering, and `run()` for execution.

### WireHookSubscription — Remote Interest

**File:** `src/hooks/hook.rs`

```rust
// octopus-cli/src/hooks/hook.rs ~line 39 — WireHookSubscription
#[derive(Debug, Clone)]
pub struct WireHookSubscription {
    pub id: String,
    pub event: HookEventKind,
    pub matcher: String,
    /// Compiled regex from `matcher`, computed when the subscription is added.
    pub compiled_matcher: Option<Regex>,
    pub timeout: u64,
}
```

When a client connects via the wire protocol, it can subscribe to hooks remotely. The server builds a `WireHook` from each subscription and adds it to the engine alongside any local `CommandHook`s.

## 3.3 The HookEngine

**File:** `src/hooks/engine.rs`

The `HookEngine` is the central dispatcher. It maintains one unified index:

```rust
// octopus-cli/src/hooks/engine.rs ~line 55 — HookEngine (abbreviated)
pub struct HookEngine {
    by_event: HashMap<HookEventKind, Vec<Box<dyn Hook>>>,
    cwd: Option<PathBuf>,
    callbacks: HookCallbacks,
}
```

### Registration

```rust
// octopus-cli/src/hooks/engine.rs ~line 100 — Registration API
engine.add_hooks(vec![hook_def]);          // server-side from config
engine.add_wire_subscriptions(vec![sub]);  // remote subscriptions
```

Both methods wrap the input in `Box<dyn Hook>` (`CommandHook` or `WireHook`) and group by `HookEventKind` for O(1) lookup.

### Trigger Flow

```
Core calls engine.trigger(HookEvent::PreToolUse { ... })
                    │
                    ▼
        ┌─────────────────────────┐
        │ 1. Derive HookEventKind │ ──O(1) HashMap lookup
        │ 2. Derive matcher_value │ ──from event payload (e.g., tool_name)
        │ 3. Wrap in Arc          │ ──Avoid cloning payload per hook
        │ 4. Pre-serialize JSON   │ ──Avoid re-serializing per wire hook
        │ 5. Match by kind        │ ──O(1) HashMap lookup
        │ 6. Filter by regex      │ ──Use pre-compiled Regex::is_match
        │ 7. Run in parallel      │ ──tokio::spawn + try_join_all
        │ 8. Aggregate            │ ──block if ANY result.action == Block
        │ 9. Cleanup callbacks    │ ──on_wire_hook_done removes stale handles
        └─────────────────────────┘
```

## 3.4 Payload Construction

In Rust, payloads are constructed by calling typed constructors on `HookEvent`:

```rust
// octopus-cli/src/hooks/event.rs ~line 109 — Payload construction
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
