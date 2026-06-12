# 5. Hook Engine Internals

The `HookEngine` is the brain of the hook system. It decides *which* hooks run, *when* they run, and *how* their results are combined. This section dissects the Rust implementation in `src/hooks/engine.rs`, with Python references where the designs diverge.

## 5.1 Data Structures

```rust
// octopus-cli/src/hooks/engine.rs ~line 55 — HookEngine
pub struct HookEngine {
    by_event: HashMap<HookEventKind, Vec<Box<dyn Hook>>>,
    cwd: Option<PathBuf>,
    callbacks: HookCallbacks,
}
```

The engine maintains a **single unified index**:
- `by_event`: maps `HookEventKind::PreToolUse` → list of `Box<dyn Hook>` objects.

Each entry in the list can be either a `CommandHook` (local shell command) or a `WireHook` (remote client subscription). They share the same runtime trait, so the engine does not need separate code paths for matching, dispatch, or aggregation.

**Python comparison:** The Python engine used `dict[str, list[HookDef]]` — string keys instead of enum keys. The lookup was `self._by_event.get("PreToolUse")`. Rust uses a typed `HookEventKind` key.

## 5.2 Index Building

When hooks are registered, they are wrapped in `Box<dyn Hook>` and grouped by kind:

```rust
// octopus-cli/src/hooks/engine.rs ~line 100 — add_hooks
pub fn add_hooks(&mut self, hooks: Vec<HookDef>) {
    for mut def in hooks {
        if def.compiled_matcher.is_none() {
            def.compile_matcher();
        }
        let hook = Box::new(CommandHook::new(&def));
        self.by_event
            .entry(hook.kind())
            .or_default()
            .push(hook);
    }
}
```

```rust
// octopus-cli/src/hooks/engine.rs ~line 115 — add_wire_subscriptions
pub fn add_wire_subscriptions(&mut self, mut subs: Vec<WireHookSubscription>) {
    for s in &mut subs {
        if s.compiled_matcher.is_none() {
            s.compiled_matcher = Regex::new(&s.matcher).ok();
        }
    }
    for s in subs {
        let hook = Box::new(WireHook::new(&s));
        self.by_event
            .entry(hook.kind())
            .or_default()
            .push(hook);
    }
}
```

This is simple but effective: it trades a small amount of setup time for O(1) event lookup at trigger time. Because `HookEventKind` is `Copy` and contains no runtime data, indexing is cheap.

## 5.3 The Hook Trait

Both hook sources implement the same trait:

```rust
// octopus-cli/src/hooks/hook.rs ~line 65 — Hook trait
#[async_trait::async_trait]
pub trait Hook: Send + Sync + std::fmt::Debug + HookClone {
    fn kind(&self) -> HookEventKind;
    fn matcher(&self) -> Option<&Regex>;
    fn source(&self) -> &'static str;
    fn command(&self) -> Option<&str> { None }
    async fn run(&self, event: &HookEvent, ctx: &HookRunContext) -> HookResult;
}
```

- `kind()` provides the registry key.
- `matcher()` returns `None` for "match everything" or a compiled regex for filtering.
- `source()` returns `"server"` or `"wire"` for diagnostics.
- `command()` returns the shell command for server-side hooks; wire hooks return `None`.
- `run()` executes the hook for a concrete event.

**Python comparison:** Python had separate code paths in `trigger()` for local and remote hooks. Rust unifies them behind one trait.

## 5.4 Matching with Compiled Regexes

The engine filters hooks by calling `Regex::is_match` on the event's natural matcher field:

```rust
// octopus-cli/src/hooks/engine.rs ~line 120 — matching loop
for h in self.by_event.get(&kind).into_iter().flatten() {
    // Server-side hooks are deduplicated by command string.
    if let Some(cmd) = h.command() {
        if seen_commands.contains(cmd) {
            continue;
        }
        seen_commands.insert(cmd.to_string());
    }
    match h.matcher() {
        None => matched.push(h),
        Some(re) if re.is_match(&matcher_value) => matched.push(h),
        _ => {}
    }
}
```

### Regex Behavior

- `matcher = ""` (or omitted) → matches everything (`None` stored in the hook).
- `matcher = "shell"` → matches exactly the string `shell` anywhere in the matcher field.
- `matcher = "shell|bash"` → matches either `shell` or `bash`.
- Invalid regex → logged once at compilation time; treated as no-match.

The matcher value is derived from the concrete `HookEvent`:

```rust
// octopus-cli/src/hooks/event.rs ~line 286 — matcher_value
pub fn matcher_value(&self) -> Option<&str> {
    match self {
        HookEvent::PreToolUse { tool_name, .. } => Some(tool_name),
        HookEvent::UserPromptSubmit { prompt, .. } => Some(prompt),
        HookEvent::Stop { .. } => None,
        // ... one arm per variant
    }
}
```

**Python comparison:** Python's matching called `re.search(pattern, value)` on every trigger, compiling the regex fresh each time:

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
// octopus-cli/src/hooks/engine.rs ~line 124 — Deduplication
if let Some(cmd) = h.command() {
    if seen_commands.contains(cmd) {
        continue;
    }
    seen_commands.insert(cmd.to_string());
}
```

Why? A user might accidentally register the same script twice in `config.toml`. Deduplication prevents double-execution.

Wire subscriptions are **not** deduplicated because each subscription comes from a different client and may return a different decision. The `command()` method returns `None` for wire hooks, so the deduplication check is skipped.

## 5.6 The Trigger Method (Detailed)

```rust
// octopus-cli/src/hooks/engine.rs ~line 110 — trigger
pub async fn trigger(&self, event: HookEvent) -> Vec<HookResult> {
    let kind = event.kind();
    let matcher_value = event.matcher_value().unwrap_or("").to_string();
    let event = Arc::new(event);
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
// octopus-cli/src/hooks/engine.rs ~line 145 — Arc optimization
let event = Arc::new(event);
// ...
let event = Arc::clone(&event);  // cheap refcount bump
hook.run(&event, &ctx).await
```

**Python comparison:** Python passed dictionaries by reference, but each `asyncio.create_task` closure captured variables from the enclosing scope. Because Python closures capture by reference, the dict was shared — but if the dict was mutated later, all tasks would see the mutation. Rust's `Arc` makes the sharing explicit and safe.

### Pre-serialization Optimization

Wire hooks serialize the event once per `run()` call. Because each `WireHook` runs independently, serialization happens once per matched wire hook. If multiple wire hooks match the same event, each serializes its own copy; this is acceptable because wire hooks are rare compared to server-side hooks.

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
// octopus-cli/src/hooks/engine.rs ~line 170 — Aggregation
let mut action = HookAction::Allow;
for r in &results {
    if let HookAction::Block(ref reason) = r.action {
        action = HookAction::Block(reason.clone());
        tracing::warn!("Hook blocked {} (matcher={}): {}", kind, matcher_value, reason);
        break;
    }
}
```

**Block wins over allow**: Even if 9 hooks say `allow` and 1 says `block`, the tool is blocked.

## 5.9 Wire Hook Cleanup

The `on_wire_hook_done` callback solves a leak that existed in the Python version:

```rust
// octopus-cli/src/hooks/hook.rs ~line 145 — Wire hook cleanup
let on_done = ctx.callbacks.on_wire_hook_done.clone();
// ...
let result = match tokio::time::timeout(..., rx).await { ... };
if let Some(ref cb) = on_done {
    cb(&handle_id);  // ← cleanup even on timeout!
}
result
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
- `trigger()` only reads shared state (`by_event`) and spawns independent tasks.
- The `on_wire_hook` callback uses `Arc<Mutex<...>>` for shared mutable state in the wire server.
