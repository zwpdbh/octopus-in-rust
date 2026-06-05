# Tour 10: The Hook System — Gates and Extension Points

> *"Every doorway in this building can have a guard. Some guards are scripts on the server; others live in the IDE across the wire. All ask the same question: may I pass?"*

Welcome to the **Hook System** — the building's security checkpoint network. Unlike the Security Desk (Tour 4) where users answer Y/N/A prompts in real time, hooks are **automated, programmable gates** that run before or after specific events. They let users enforce policy, log activity, or integrate with external systems — all without modifying core code.

In this tour, we'll explore:
1. The **event taxonomy** — 11 extension points where hooks can attach
2. The **discriminant trick** — how Rust's type system lets config files match runtime events
3. The **hook engine** — parallel evaluation with "block wins" aggregation
4. **Server-side hooks** — shell scripts executed locally
5. **Wire hooks** — client-side subscriptions over JSON-RPC
6. The **gate pattern** — why hooks are allow/block only (no overwrite)

---

## 🎯 What Is a Hook?

A hook is a **function that runs at an extension point**. It receives the event context as JSON and returns a decision: **allow** (proceed) or **block** (abort).

```
┌─────────────────┐     ┌─────────────┐     ┌─────────────────┐
│   Event Fires   │────▶│  Hook Runs  │────▶│ Allow → Continue│
│ (e.g. PreToolUse)│     │ (shell/wire)│     │ Block → Abort   │
└─────────────────┘     └─────────────┘     └─────────────────┘
```

There are **two sources** of hooks:

| Source | Registration | Execution | Use Case |
|--------|-------------|-----------|----------|
| **Server-side** | `~/.kimi/config.toml` | Local shell command | Personal policy scripts |
| **Wire (client-side)** | `initialize` JSON-RPC | Remote IDE/editor | Team policy, UI integration |

Both sources feed into the same engine and follow the same rules.

---

## 📋 The Event Taxonomy: `HookEvent`

File: `octopus-cli/src/hooks/event.rs` (~553 lines)

Every extension point is a variant of a single enum:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "hook_event_name", rename_all = "PascalCase")]
pub enum HookEvent {
    PreToolUse { session_id, cwd, tool_name, tool_input, tool_call_id },
    PostToolUse { session_id, cwd, tool_name, tool_input, tool_output, tool_call_id },
    PostToolUseFailure { session_id, cwd, tool_name, tool_input, error, tool_call_id },
    UserPromptSubmit { session_id, cwd, prompt },
    Stop { session_id, cwd, stop_hook_active },
    StopFailure { session_id, cwd, error_type, error_message },
    SessionStart { session_id, cwd, source },
    SessionEnd { session_id, cwd, reason },
    PreCompact { session_id, cwd, trigger, token_count },
    PostCompact { session_id, cwd, trigger, estimated_token_count },
    Notification { session_id, cwd, sink, notification_type, title, body, severity },
}
```

| Event | When It Fires | Matcher Value |
|-------|--------------|---------------|
| `PreToolUse` | Before a tool executes | `tool_name` (e.g. `WriteFile`) |
| `PostToolUse` | After a tool succeeds | `tool_name` |
| `PostToolUseFailure` | After a tool fails | `tool_name` |
| `UserPromptSubmit` | When the user submits input | `prompt` text |
| `Stop` | When a turn completes normally | — |
| `StopFailure` | When a turn fails | — |
| `SessionStart` | When a session begins | — |
| `SessionEnd` | When a session ends | — |
| `PreCompact` | Before context compaction | — |
| `PostCompact` | After context compaction | — |
| `Notification` | When a notification is delivered | `sink` name |

🐍 **Python's way:** Python uses the same 11 events, but they're constructed as dictionaries with string keys at call sites.

🦀 **Rust's way:** A single enum where each variant carries exactly the data it needs. Serialization generates the same JSON Python produced, but construction is type-checked.

✨ **Where Rust shines:** **Adding a field to `PreToolUse` is a compile error** at every construction site until you provide it. In Python, a missing key is a runtime `KeyError` in the hook script's stdin.

---

## 🔑 The Discriminant Trick: Same Type, Two Jobs

Here's a subtle but critical design choice. The `HookEvent` enum serves **two purposes simultaneously**:

1. **As a typed payload** — serialized to JSON and sent to hook scripts
2. **As a HashMap key** — indexing registered hooks by event type in the engine

These two purposes have conflicting equality semantics:
- As a payload: `PreToolUse { tool_name: "A" }` ≠ `PreToolUse { tool_name: "B" }`
- As a key: `PreToolUse { ..any.. }` == `PreToolUse { ..any.. }` (we just want to group by event type)

Rust solves this with **discriminant-only equality**:

```rust
impl PartialEq for HookEvent {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl std::hash::Hash for HookEvent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
    }
}
```

This means:

```rust
let a = HookEvent::PreToolUse { tool_name: "WriteFile".into(), /* ... */ };
let b = HookEvent::PreToolUse { tool_name: "ReadFile".into(), /* ... */ };
assert_eq!(a, b);  // true! Same discriminant.
```

### Why This Matters: Config Matching

When you write this in `config.toml`:

```toml
[[hooks]]
event = "PreToolUse"
pattern = "write_file|str_replace_file"
command = "./scripts/ask_before_write.sh"
```

The config loader deserializes `event = "PreToolUse"` into a `HookEvent::PreToolUse` with **empty strings** for all fields:

```rust
HookEvent::PreToolUse {
    session_id: "",
    cwd: "",
    tool_name: "",
    tool_input: {},
    tool_call_id: "",
}
```

At runtime, when `WriteFile` is about to run, the engine fires a `PreToolUse` with **real data**. Thanks to discriminant equality, the config hook (empty payload) matches the runtime event (real payload) perfectly.

🐍 **Python's way:** Python stores hooks in a dict keyed by string event names (`"PreToolUse"`). The string key naturally ignores payload data.

🦀 **Rust's way:** No parallel string-key dict. The same type does both jobs. The `discriminant_serde` module handles config-file round-tripping:

```rust
// Serialize for config: just the variant name
"PreToolUse"

// Deserialize from config: reconstruct variant with empty defaults
HookEvent::PreToolUse { session_id: "", cwd: "", ... }
```

✨ **Where Rust shines:** **One type, zero drift.** If you rename a variant, the compiler updates every config deserialization site, every HashMap lookup, and every payload construction. In Python, renaming `"PreToolUse"` is a global search-and-replace with no safety net.

---

## ⚙️ The Hook Engine: Parallel Evaluation

File: `octopus-cli/src/hooks/engine.rs` (~469 lines)

The `HookEngine` is the central dispatcher. It maintains two indexes:

```rust
pub struct HookEngine {
    hooks: Vec<HookDef>,                    // server-side (config.toml)
    wire_subs: Vec<WireHookSubscription>,   // client-side (wire initialize)
    by_event: HashMap<HookEvent, Vec<HookDef>>,
    wire_by_event: HashMap<HookEvent, Vec<WireHookSubscription>>,
    // ... callbacks for telemetry and wire dispatch
}
```

### The Trigger Flow

When an event fires, `engine.trigger(event, matcher_value)` does this:

```rust
pub async fn trigger(&self, event: HookEvent, matcher_value: &str) -> Vec<HookResult> {
    // 1. Match server-side hooks by event + regex
    let server_matched: Vec<&HookDef> = /* ... */;

    // 2. Match wire subscriptions by event + regex
    let wire_matched: Vec<&WireHookSubscription> = /* ... */;

    // 3. Run all matches in parallel via tokio::spawn
    let mut tasks: Vec<JoinHandle<HookResult>> = Vec::new();
    for h in server_matched {
        tasks.push(tokio::spawn(run_hook(...)));
    }
    for s in wire_matched {
        tasks.push(tokio::spawn(dispatch_wire_hook(...)));
    }

    let results = futures::future::try_join_all(tasks).await?;

    // 4. Aggregate: BLOCK wins
    for r in &results {
        if let HookAction::Block(reason) = r.action {
            return vec![HookResult::block(reason)];
        }
    }
    results
}
```

Key behaviors:
- **Parallel, not sequential.** All matching hooks run concurrently.
- **Block wins.** If any hook says "block," the operation is aborted. No majority vote, no weighted scoring.
- **Fail-open.** If a hook crashes, times out, or returns malformed output, it defaults to `Allow`.

🐍 **Python's way:** Same architecture — `asyncio.gather()` for parallelism, "block wins" aggregation.

🦀 **Rust's way:** `tokio::spawn` + `try_join_all`. The engine is `Clone` (callbacks are cleared on clone) so it can be moved into spawned tasks safely.

---

## 🖥️ Server-Side Hooks: Shell Scripts

File: `octopus-cli/src/hooks/runner.rs` (~167 lines)

Server-side hooks are shell commands configured in `~/.kimi/config.toml`. The runner:

1. Serializes the `HookEvent` to JSON
2. Spawns `sh -c "<command>"` with the JSON on stdin
3. Waits for output with a timeout
4. Parses the decision

### Decision Protocol

A hook script can signal its decision in **two ways**:

**Exit code (simple):**
- Exit `0` → Allow
- Exit `2` → Block (stderr becomes the reason)

**Structured JSON (rich):**
```json
{
  "hookSpecificOutput": {
    "permissionDecision": "deny",
    "permissionDecisionReason": "Writing to Cargo.toml requires review"
  }
}
```

```rust
// Exit 2 = block with stderr as reason
if exit_code == 2 {
    return HookResult::block(stderr.trim());
}

// Exit 0 + JSON stdout = structured decision
if exit_code == 0 && !stdout.is_empty() {
    if let Ok(json) = serde_json::from_str(&stdout) {
        if json.hookSpecificOutput.permissionDecision == "deny" {
            return HookResult::block(json.hookSpecificOutput.permissionDecisionReason);
        }
    }
}

// Everything else = allow
HookResult::allow()
```

### Fail-Open Philosophy

Notice every error path returns `Allow`:
- Serialization failed? → Allow
- Spawn failed? → Allow
- Timed out? → Allow (with `timed_out: true`)
- Invalid JSON? → Allow

This is **intentional**. Hooks are safety nets, not single points of failure. A misconfigured hook should never brick the agent.

🐍 **Python's way:** Same fail-open behavior. Same exit-code + JSON decision protocol.

🦀 **Rust's way:** Native `tokio::process::Command` with async I/O. No Python interpreter overhead per hook invocation.

---

## 🌐 Wire Hooks: Client-Side Over JSON-RPC

File: `octopus-cli/src/wire_server/mod.rs` (~703 lines)

Wire hooks let an **IDE or editor** (the wire client) register subscriptions and receive hook requests in real time. This is how VS Code, JetBrains, or a custom GUI can enforce policy or show UI dialogs.

### Registration

During wire initialization, the client sends hook subscriptions:

```json
{
  "jsonrpc": "2.0",
  "method": "initialize",
  "id": "init-1",
  "params": {
    "hooks": [
      {
        "id": "hook-1",
        "event": "PreToolUse",
        "matcher": "WriteFile|StrReplaceFile",
        "timeout": 30
      }
    ]
  }
}
```

The server parses these into `WireHookSubscription` structs and adds them to the engine:

```rust
for h in msg.params.hooks {
    match parse_hook_event(&h.event) {
        Some(event) => subs.push(WireHookSubscription {
            id: h.id,
            event,
            matcher: h.matcher,
            timeout: h.timeout,
        }),
        None => warn!("Ignoring unknown hook event: {}", h.event),
    }
}
soul.hook_engine.add_wire_subscriptions(subs);
```

### Request/Response Lifecycle

When a matching event fires, the engine creates a `WireHookHandle`:

```rust
pub struct WireHookHandle {
    pub id: String,                    // unique request ID
    pub subscription_id: String,       // which subscription matched
    pub event_name: String,            // "PreToolUse"
    pub target: String,                // "WriteFile" (the matcher value)
    pub input_data: serde_json::Value, // full event payload
    tx: Option<oneshot::Sender<HookResult>>, // resolver channel
}
```

The wire server:
1. **Sends a `HookRequest`** to the client over JSON-RPC
2. **Waits on a `oneshot::Receiver`** with a timeout
3. **Receives the client's response** and resolves the handle

```rust
let on_wire_hook: OnWireHook = Box::new(move |handle: WireHookHandle| {
    Box::pin(async move {
        // 1. Send HookRequest to client
        let request = HookRequest { /* ... */ };
        pending.lock().await.insert(request.id.clone(), PendingRequest::Hook(handle));
        write_tx.send(JSONRPCRequestMessage::new(request.id, request));

        // 2. Client receives it, shows UI, user clicks "Block"
        // 3. Client sends JSON-RPC response back
        // 4. handle_response() routes it to the pending Hook request
        // 5. handle.resolve(HookAction::Block(reason)) sends result on the oneshot channel
    })
});
```

The engine side uses `tokio::time::timeout` to prevent hung clients from blocking the agent forever:

```rust
tasks.push(tokio::spawn(async move {
    cb_future.await;  // let the callback send the request
    match tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            warn!("Wire hook resolver dropped without resolving");
            HookResult::allow()
        }
        Err(_) => {
            warn!("Wire hook timed out");
            HookResult { action: Allow, timed_out: true, .. }
        }
    }
}));
```

### Response Routing

When the client replies, the wire server's `handle_response()` routes it:

```rust
async fn handle_response(&self, resp: JSONRPCClientResponse) {
    let pending = self.pending_requests.lock().await.remove(&id);
    match pending {
        Some(PendingRequest::Hook(handle)) => {
            if let Ok(body) = serde_json::from_value::<HookResponse>(result) {
                let action = if body.action == "block" {
                    HookAction::Block(body.reason)
                } else {
                    HookAction::Allow
                };
                handle.resolve(action);  // wakes the oneshot receiver
            }
        }
        // ... approval requests handled similarly
    }
}
```

🐍 **Python's way:** Python's wire server uses the same `HookRequest` / `HookResponse` protocol. The IDE registers subscriptions via `initialize`, receives requests, and sends responses with the same JSON shape.

🦀 **Rust's way:** `tokio::sync::oneshot` for request/response pairing instead of Python's dict-based pending request registry. The `PendingRequest` enum makes it impossible to accidentally route a hook response to an approval handler — the compiler enforces it.

---

## 🚪 The Gate Pattern: Allow vs. Block

A critical design constraint: **hooks are gates, not transformers.**

They can only:
- ✅ **Allow** — proceed with the original operation unchanged
- ❌ **Block** — abort with a reason

They **cannot**:
- ❌ Modify tool arguments
- ❌ Replace the tool output
- ❌ Inject additional behavior

This is the **gate pattern** (also known as the "admission controller" pattern):

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Original   │────▶│   HOOK      │────▶│  Original   │
│  Arguments  │     │ (read-only) │     │  Arguments  │
└─────────────┘     └─────────────┘     └─────────────┘
                           │
                     ┌─────┴─────┐
                     ▼           ▼
                  Allow       Block
```

Why no overwrite? Three reasons:
1. **Predictability.** The LLM generated those arguments for a reason. Silent modification creates debugging nightmares.
2. **Type safety.** Changing `path` from `"/tmp/a"` to `"/tmp/b"` is easy in Python; in Rust, the tool's typed `Params` struct would need every field to be `mut`, complicating the entire tool trait.
3. **Simplicity.** Gates are easy to reason about. Transformers require ordering rules, conflict resolution, and rollback semantics.

If you need to modify behavior, **use a custom tool** or **write a wrapper script** — don't try to mutate inside a hook.

🐍 **Python's way:** Same gate-only semantics. The original Python implementation never used hook stdout to overwrite arguments.

🦀 **Rust's way:** Enforced by the `HookAction` enum having only two variants. There's simply no `Modify` or `Replace` variant to construct.

---

## 🔗 Where Hooks Sit in the Tool Pipeline

Recall from Tour 3 (The Tool Shed) that every tool call passes through a pipeline:

```
1. Deduplication check
        ↓
2. PreToolUse hook ← YOU ARE HERE
        ↓
3. Approval check (Y/N/A prompt)
        ↓
4. TOOL EXECUTES
        ↓
5. PostToolUse / PostToolUseFailure hook ← OR HERE
        ↓
6. Telemetry
```

Hooks run **before approval**. This is important: a hook can block a tool call before the user ever sees a Y/N/A prompt. If you have both a hook and approval enabled:

1. Hook says "block" → Tool aborts immediately, no prompt shown
2. Hook says "allow" → Approval prompt may still appear
3. No hook matches → Approval prompt appears (if not yolo/afk)

This ordering lets hooks enforce **hard policy** ("never delete files") while approval handles **discretionary judgment** ("this specific write looks risky").

---

## 🎁 Souvenir Shop: What to Remember

1. **11 events, one enum.** `HookEvent` covers every extension point. Adding a new event is a single variant — the compiler guides you to all match sites.

2. **Discriminant equality is the secret sauce.** It lets config-loaded hooks (empty payloads) match runtime events (real payloads) without a parallel string-key dictionary.

3. **Two sources, one engine.** Server-side (shell) and wire (client) hooks run through the same `HookEngine::trigger()`. Both are evaluated in parallel.

4. **Block wins, fail-open.** If any hook blocks, the operation aborts. If a hook crashes or times out, it defaults to allow — hooks are safety nets, not straitjackets.

5. **Gates, not transformers.** Hooks can only allow or block. They cannot modify arguments or outputs. This is a feature, not a limitation — it keeps the mental model simple.

6. **Wire hooks use oneshot channels.** The `WireHookHandle` carries a `oneshot::Sender<HookResult>` that the wire server resolves when the client responds. Timeouts prevent hung clients from blocking the agent.

7. **Hooks run before approval.** A blocking hook prevents the approval prompt from ever appearing. Use hooks for hard policy; use approval for discretionary judgment.

---

## 🚶 Next Stop

You've now seen the Hook System in detail — the building's programmable security checkpoints. But what about the approvals that happen when no hook blocks? That's the domain of the **Security Desk** (Tour 4), where users answer Y/N/A prompts in real time.

If you've already visited the Security Desk, congratulations — you've seen every gate in the building!

→ [Back to Index](./00-index.md)
