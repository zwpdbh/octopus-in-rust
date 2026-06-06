# 8. Lessons from the Rewrite

This section is a before-and-after comparison. It shows **concrete improvements** the Rust port made over the Python original, and explains the reasoning behind each change. If you are porting a Python system to Rust (or designing a hook system from scratch), these are the design decisions worth copying.

## 8.1 Guiding Principles from AGENTS.md

The project mandates three rules that directly shaped the hook system:

1. **Model states with enums and match on them** — never use string literals for event types.
2. **Deserialize JSON into typed structs** — never use `serde_json::Value` and manual indexing.
3. **Use strong enums for channel and IPC messages** — never use `String` or raw bytes as carrier types.

These rules are in direct tension with the Python implementation, which used:
- `HookEventType = Literal["PreToolUse", ...]` (string literals).
- `dict[str, Any]` payloads built by helper functions.
- Trial-and-error deserialization for wire events.

## 8.2 Replace String Literals with a Rust Enum

### Python (before)

```python
HookEventType = Literal[
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    # ...
]

def trigger(self, event: HookEventType, ...):
    candidates = self._by_event.get(event, [])  # string key lookup
```

### Rust (after)

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "hook_event_name", rename_all = "PascalCase")]
pub enum HookEvent {
    PreToolUse { session_id: String, cwd: String, tool_name: String, ... },
    PostToolUse { ... },
    PostToolUseFailure { ... },
    // ...
}

impl PartialEq for HookEvent {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
```

**Why this is better:**
- **Exhaustiveness:** `match event { ... }` forces handling every variant. Adding a new event is a compile error until all `match` sites are updated.
- **No typos:** `HookEvent::PreToolUse` is checked at compile time. `"PreToolUse"` is not.
- **Refactoring:** Rename a variant and the compiler shows every use site.
- **Dual use:** The same enum acts as a payload (full data) and a dictionary key (discriminant-only equality).

## 8.3 Replace dict[str, Any] with Typed Payloads

### Python (before)

```python
def pre_tool_use(session_id: str, cwd: str, tool_name: str, ...) -> dict[str, Any]:
    return {
        **_base("PreToolUse", session_id, cwd),
        "tool_name": tool_name,
        "tool_input": tool_input,
        "tool_call_id": tool_call_id,
    }
```

### Rust (after)

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "hook_event_name", rename_all = "PascalCase")]
pub enum HookEvent {
    PreToolUse {
        session_id: String,
        cwd: String,
        tool_name: String,
        tool_input: HashMap<String, serde_json::Value>,
        tool_call_id: String,
    },
    // ...
}
```

Construction is explicit and type-checked:

```rust
let event = HookEvent::PreToolUse {
    session_id: session_id.into(),
    cwd: cwd.into(),
    tool_name: tool_name.into(),
    tool_input: tool_input.clone(),
    tool_call_id: tool_call_id.into(),
};
```

**Why this is better:**
- Adding a field to `PreToolUse` is a compile error at every construction site until it is provided.
- A `PostToolUseFailure` payload cannot be accidentally passed where `PreToolUse` is expected.
- `serde` generates the exact same JSON the old `dict` builders produced.

## 8.4 Replace Trial-and-Error Wire Deserialization with a Strong Enum

### Python (before)

```python
# Consumer deserializes by trial-and-error
if let Ok(req) = serde_json::from_value::<ApprovalRequestEvent>(value.clone()):
    self.pending_approval = Some(req)
elif let Ok(text) = serde_json::from_value::<TextPart>(value):
    self.append_text(text.text)
```

### Rust (after)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WireEvent {
    TextPart(TextPart),
    TurnBegin(TurnBegin),
    HookRequest(HookRequest),
    HookResponse(HookResponse),
    HookTriggered(HookTriggered),
    HookResolved(HookResolved),
    // ...
}
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

**Why this is better:**
- Adding a new `WireEvent` variant is a compile error across all consumers.
- No runtime trial-and-error overhead.
- Rename a variant and the compiler shows every producer and consumer.

## 8.5 Compile Regexes Once, Not on Every Trigger

### Python (before)

```python
# engine.py — called on EVERY trigger
match_regex(pattern, value):
    return re.search(pattern, value)  # compiles fresh every time
```

### Rust (after)

```rust
// config.rs — called once at load time
pub fn compile_matcher(&mut self) {
    self.compiled_matcher = self.matcher.as_ref().and_then(|p| {
        match Regex::new(p) {
            Ok(re) => Some(re),
            Err(e) => {
                tracing::warn!("Invalid regex in hook matcher '{}': {}", p, e);
                None
            }
        }
    });
}

// engine.rs — called at trigger time
fn match_regex(compiled: Option<&Regex>, pattern: &str, value: &str) -> bool {
    if pattern.is_empty() { return true; }
    match compiled {
        Some(re) => re.is_match(value),
        None => false,
    }
}
```

**Why this is better:**
- Regex compilation is expensive. In the Python version, every tool call recompiled every hook's regex.
- In the Rust version, compilation happens once at config load. At trigger time, only `is_match` is called.

## 8.6 Eliminate Per-Hook Payload Cloning with Arc

### Before (both Python and early Rust)

```rust
// Cloned once per matched hook
let event = event.clone();
tasks.push(tokio::spawn(async move {
    run_hook(&command, &event, ...).await
}));
```

### After (Rust)

```rust
let event = Arc::new(event);
// ...
let event = Arc::clone(&event);  // cheap refcount bump
tasks.push(tokio::spawn(async move {
    run_hook(&command, &*event, ...).await
}));
```

**Why this is better:**
- `HookEvent` contains `HashMap<String, Value>` for `tool_input`. Cloning it for every matching hook is O(n) in map size.
- `Arc::clone` is O(1) and shares the data immutably.

## 8.7 Eliminate Wire Handle Leaks with Explicit Cleanup

### Python (before)

```python
# Wire hook stored in pending_requests
pending[handle.id] = PendingRequest::Hook(handle)

# Removed ONLY on client response
pending.pop(id, None)

# If client never responds: leak until GC
```

### Rust (after)

```rust
// Engine calls on_done even on timeout/drop
let on_done = self.on_wire_hook_done.clone();
tasks.push(tokio::spawn(async move {
    let result = match tokio::time::timeout(..., rx).await { ... };
    if let Some(ref cb) = on_done {
        cb(&handle_id);  // ← cleanup even on timeout
    }
    result
}));

// Wire server provides the cleanup callback
let on_done = Arc::new(move |id: &str| {
    let pending = pending_cleanup.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        pending.lock().await.remove(&id);
    });
});
```

**Why this is better:**
- Python's GC would eventually collect leaked handles, but there was no deterministic cleanup.
- Rust has no GC, so leaks are permanent. The `on_wire_hook_done` callback guarantees cleanup on every code path.

## 8.8 Replace Manual Value Indexing with Typed Deserialization

### Python (before)

```python
parsed = json.loads(stdout)
if parsed.get("hookSpecificOutput", {}).get("permissionDecision") == "deny":
    reason = parsed.get("hookSpecificOutput", {}).get("permissionDecisionReason", "")
```

### Rust (after)

```rust
#[derive(Debug, Deserialize)]
struct HookStdout {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(Debug, Deserialize)]
struct HookSpecificOutput {
    #[serde(rename = "permissionDecision")]
    permission_decision: Option<String>,
    #[serde(rename = "permissionDecisionReason")]
    permission_decision_reason: Option<String>,
}

if let Ok(parsed) = serde_json::from_str::<HookStdout>(&stdout) {
    if let Some(ref output) = parsed.hook_specific_output {
        if output.permission_decision.as_deref() == Some("deny") {
            let reason = output.permission_decision_reason.clone().unwrap_or_default();
            return HookResult::block(reason);
        }
    }
}
```

**Why this is better:**
- A typo in `permissionDecision` is a compile error, not a silent `None`.
- Adding a new required field forces updates at every deserialization site.
- `hookSpecificOutput.permissionDecision` is self-documenting; `body["hookSpecificOutput"].as_object()?.get("permissionDecision")` is not.

## 8.9 Dead Code Elimination

The Python version defined `SessionStart` and `SessionEnd` hooks that were never triggered. They existed in the enum, config schema, and tests but had **zero call sites**.

In the Rust rewrite, these variants were removed entirely. The compiler would have flagged unused variants if they were never constructed, but because they were part of a public enum used in deserialization, they persisted as "zombie code."

**Lesson:** If a variant is deserializable but never constructed at runtime, it is dead code. Remove it to reduce confusion and maintenance burden.

## 8.10 Summary Table

| Concern | Python (kimi-cli) | Rust (octopus-cli) |
|---------|-------------------|-------------------|
| **Event type** | `Literal["..."]` | `enum HookEvent` |
| **Payload** | `dict[str, Any]` | `enum HookEvent` with variant data |
| **Wire carrier** | Pydantic model / dict | `enum WireEvent` with `#[serde(untagged)]` |
| **Subprocess** | `asyncio.create_subprocess_shell` | `tokio::process::Command` |
| **Regex** | `re.search` per trigger | `Regex::new` once, `is_match` per trigger |
| **Clone cost** | Dict shared by reference (with mutation risk) | `Arc<HookEvent>` — explicit, cheap, safe |
| **Wire cleanup** | None — leaked until GC | `on_wire_hook_done` callback |
| **Stdout parsing** | Manual `dict.get` | Typed `Deserialize` structs |
| **Result** | `HookResult(action="block")` | `HookResult { action: HookAction::Block }` |
| **Aggregation** | `for r in results: if r.action == "block"` | `results.iter().any(|r| matches!(r.action, HookAction::Block))` |
| **Fail-open** | `except: return HookResult("allow")` | `Err(_) => HookResult { action: Allow, ... }` |
