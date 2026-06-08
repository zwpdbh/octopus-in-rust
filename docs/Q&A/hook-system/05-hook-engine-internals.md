# 5. Hook Engine Internals

The `HookEngine` is the brain of the hook system. It decides *which* hooks run, *when* they run, and *how* their results are combined. This section dissects the Rust implementation in `src/hooks/engine.rs`, with Python references where the designs diverge.

## 5.1 Data Structures

```rust
// octopus-cli/src/hooks/engine.rs ~line 59 — HookEngine
pub struct HookEngine {
    hooks: Vec<HookDef>,
    wire_subs: Vec<WireHookSubscription>,
    cwd: Option<PathBuf>,
    on_triggered: Option<OnTriggered>,
    on_resolved: Option<OnResolved>,
    on_wire_hook: Option<OnWireHook>,
    on_wire_hook_done: Option<OnWireHookDone>,
    by_event: HashMap<HookEvent, Vec<HookDef>>,
    wire_by_event: HashMap<HookEvent, Vec<WireHookSubscription>>,
}
```

The engine maintains **two indexes**:
- `by_event`: maps `HookEvent::PreToolUse` → list of local `HookDef` objects.
- `wire_by_event`: maps `HookEvent::PreToolUse` → list of remote `WireHookSubscription` objects.

**Python comparison:** The Python engine used `dict[str, list[HookDef]]` — string keys instead of enum keys. The lookup was `self._by_event.get("PreToolUse")`.

## 5.2 Index Rebuilding

```rust
// octopus-cli/src/hooks/engine.rs ~line 165 — rebuild_index
fn rebuild_index(&mut self) {
    self.by_event.clear();
    for h in &self.hooks {
        self.by_event
            .entry(h.event.clone())
            .or_default()
            .push(h.clone());
    }
    self.wire_by_event.clear();
    for s in &self.wire_subs {
        self.wire_by_event
            .entry(s.event.clone())
            .or_default()
            .push(s.clone());
    }
}
```

This is simple but effective: it trades a small amount of startup time for O(1) event lookup at trigger time.

Because `HookEvent` equality is **discriminant-only**, `h.event.clone()` is cheap — it only clones the enum discriminant and a few empty strings (for config-loaded hooks) or the actual payload (for runtime events). In practice, the `Arc<HookEvent>` optimization in `trigger()` makes this cost negligible.

## 5.3 Registration API

### Adding Server-Side Hooks

```rust
// octopus-cli/src/hooks/engine.rs ~line 108 — add_hooks
pub fn add_hooks(&mut self, hooks: Vec<HookDef>) {
    self.hooks.extend(hooks);
    self.rebuild_index();
}
```

Called during startup after parsing `config.toml`. The hooks are already compiled by `HookDef::compile_matcher()` during config loading.

### Adding Wire Subscriptions

```rust
// octopus-cli/src/hooks/engine.rs ~line 113 — add_wire_subscriptions
pub fn add_wire_subscriptions(&mut self, mut subs: Vec<WireHookSubscription>) {
    for s in &mut subs {
        s.compiled_matcher = Regex::new(&s.matcher).ok();
    }
    self.wire_subs.extend(subs);
    self.rebuild_index();
}
```

Called when a wire client sends its subscription list during initialization.

**Python comparison:** Python's `add_wire_subscriptions` stored the raw string matcher and compiled it on every trigger.

## 5.4 Matching with Compiled Regexes

```rust
// octopus-cli/src/hooks/engine.rs ~line 182 — match_regex
fn match_regex(compiled: Option<&Regex>, pattern: &str, value: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    match compiled {
        Some(re) => re.is_match(value),
        None => {
            // Invalid regex was already logged when compiled; treat as no-match.
            false
        }
    }
}
```

### Regex Behavior

- `matcher = ""` → matches everything (default).
- `matcher = "shell"` → matches exactly the string `shell` anywhere in `matcher_value`.
- `matcher = "shell|bash"` → matches either `shell` or `bash`.
- Invalid regex → logged once at compilation time; treated as no-match.

**Python comparison:** Python's `match_regex` called `re.search(pattern, value)` on every trigger, compiling the regex fresh each time:

```python
match Regex::new(pattern):  # Rust compiles once
    Ok(re) => re.is_match(value)
```

```python
re.search(pattern, value)   # Python compiles every call
```

## 5.5 Deduplication

Server-side hooks are deduplicated by **command string**:

```rust
// octopus-cli/src/hooks/engine.rs ~line 200 — Deduplication
let mut seen_commands: std::collections::HashSet<String> = std::collections::HashSet::new();
let mut server_matched: Vec<&HookDef> = Vec::new();
for h in self.by_event.get(&*event).into_iter().flatten() {
    // ... regex check ...
    if seen_commands.contains(&h.command) {
        continue;
    }
    seen_commands.insert(h.command.clone());
    server_matched.push(h);
}
```

Why? A user might accidentally register the same script twice in `config.toml`. Deduplication prevents double-execution.

Wire subscriptions are **not** deduplicated because each subscription comes from a different client and may return a different decision.

## 5.6 The Trigger Method (Detailed)

```rust
// octopus-cli/src/hooks/engine.rs ~line 196 — trigger
pub async fn trigger(&self, event: HookEvent, matcher_value: &str) -> Vec<HookResult> {
    let event = Arc::new(event);
    let input_data = serde_json::to_value(&*event).unwrap_or_default();
    // ... match, dedup, spawn tasks, gather, aggregate, callbacks
}
```

### Arc Optimization

Before the fix, the code did:

```rust
// Conceptual pseudo-code: cloning once per hook
let event = event.clone();  // cloned once per hook!
```

Now it does:

```rust
// octopus-cli/src/hooks/engine.rs ~line 196 — Arc optimization
let event = Arc::new(event);
// ...
let event = Arc::clone(&event);  // cheap refcount bump
run_hook(&command, &*event, timeout, cwd.as_deref()).await
```

**Python comparison:** Python passed dictionaries by reference, but each `asyncio.create_task` closure captured variables from the enclosing scope. Because Python closures capture by reference, the dict was shared — but if the dict was mutated later, all tasks would see the mutation. Rust's `Arc` makes the sharing explicit and safe.

### Pre-serialization Optimization

Before the fix, wire hooks did:

```rust
// Conceptual pseudo-code: serializing once per wire hook
input_data: serde_json::to_value(&event).unwrap_or_default(),  // once per wire hook
```

Now the event is serialized **once** before the loop, and the `Value` is cloned for each wire hook.

## 5.7 Fail-Open Guarantee

The engine guarantees that failures never accidentally block:

| Failure Mode | Result | Reasoning |
|--------------|--------|-----------|
| Subprocess crashes | `allow` | Don't block users because a script has a bug. |
| Timeout (default 30s) | `allow` | Don't freeze the CLI because a hook is slow. |
| Invalid JSON stdout | `allow` | Malformed output is treated as non-blocking. |
| Wire client disconnects | `allow` | Don't block if the remote UI is gone. |
| Telemetry crash | Ignored | Telemetry is outside the main try/except. |

This is critical for **availability**: a broken hook should not brick the entire tool.

## 5.8 Aggregation Semantics

```rust
// octopus-cli/src/hooks/engine.rs ~line 315 — Aggregation
let mut action = HookAction::Allow;
for r in &results {
    if let HookAction::Block(ref reason) = r.action {
        action = HookAction::Block(reason.clone());
        tracing::warn!("Hook blocked {} (matcher={}): {}", event, matcher_value, reason);
        break;
    }
}
```

**Block wins over allow**: Even if 9 hooks say `allow` and 1 says `block`, the tool is blocked.

## 5.9 Wire Hook Cleanup

The `on_wire_hook_done` callback solves a leak that existed in the Python version:

```rust
// octopus-cli/src/hooks/engine.rs ~line 251 — Wire hook cleanup
let on_done = self.on_wire_hook_done.clone();
// ...
tasks.push(tokio::spawn(async move {
    let result = match tokio::time::timeout(..., rx).await { ... };
    if let Some(ref cb) = on_done {
        cb(&handle_id);  // ← cleanup even on timeout!
    }
    result
}));
```

In the wire server, this callback removes the entry from `pending_requests`:

```rust
// octopus-cli/src/wire_server/mod.rs ~line 471 — Wire server cleanup callback
let on_done = Arc::new(move |id: &str| {
    let pending = pending_cleanup.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        pending.lock().await.remove(&id);
    });
});
```

**Python comparison:** The Python version had no equivalent cleanup. If a client never responded, the `PendingRequest` stayed in the server's `_pending_requests` dict forever. Python's garbage collector would eventually collect it, but there was no explicit cleanup.

## 5.10 Thread Safety

The `HookEngine` is **not thread-safe** for mutation but is safe for concurrent triggers because:
- `add_hooks()` and `add_wire_subscriptions()` are only called during setup.
- `trigger()` only reads shared state (`by_event`, `wire_by_event`) and spawns independent tasks.
- The `on_wire_hook` callback uses `Arc<Mutex<...>>` for shared mutable state in the wire server.
