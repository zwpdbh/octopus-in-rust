# Tour 2: The Control Room — The Soul of the Machine

> _"This is where the building thinks. Every decision, every word, every tool call originates here."_

Welcome to the **Control Room** — the second floor, and the beating heart of Octopus-CLI. This is where `KimiSoul` lives. If the lobby is the building's entrance, the Control Room is its **brain**.

In this room, we'll watch the agent **observe** (read user input), **think** (call the LLM), **act** (execute tools), and **remember** (persist context). This cycle — the **ReAct loop** — is the fundamental pattern of modern AI agents.

---

## 🧠 The Central Computer: `KimiSoul`

File: `octopus-cli/src/soul/kimisoul.rs` (~1,290 lines)

The `KimiSoul` struct is the control room's main console. Every field is a piece of equipment:

```rust
// File: octopus-cli/src/soul/kimisoul.rs
pub struct KimiSoul {
    pub config: Config,          // Building blueprints
    pub session: Session,        // Current visitor's file
    pub llm: Option<LLM>,        // The external oracle (API)
    pub context: Context,        // Short-term memory
    pub toolset: Arc<KimiToolset>, // The toolbox (Tour 3)
    pub approval: Approval,      // Security officer (Tour 4)
    pub agent: Agent,            // Agent identity (name, system prompt, toolset)
    pub root_wire_hub: Option<RootWireHub>, // Intercom system (Tour 5)
    pub notification_manager: NotificationManager, // Mailroom (Tour 5)
    pub oauth: OAuthManager,     // Credential vault (Tour 4)
    pub skills: SkillRegistry,   // Employee directory (Tour 6)
    pub bg_manager: BackgroundTaskManager, // Basement workshop (Tour 6)
    // ... and more
}
```

🐍 **Python's way:** The `KimiSoul` class in `kimisoul.py` (~400 lines) plus `Soul` protocol in `__init__.py` (~304 lines). Python uses a protocol (interface) pattern — any object implementing `run()` can be a soul.

🦀 **Rust's way:** A single concrete struct. No trait objects for the core loop — the compiler **monomorphizes** everything, meaning there's zero runtime dispatch cost for the soul's methods.

✨ **Where Rust shines:** **The soul is Sized.** In Python, `soul` could be any object. In Rust, `KimiSoul` has a known size at compile time. This means it can live on the stack, be moved (not referenced), and be **cache-friendly**. The entire soul fits in L2 cache.

---

## 🔄 The Main Cycle: `run()`

```rust
// File: octopus-cli/src/soul/kimisoul.rs
pub async fn run(&mut self, user_input: &str) -> Result<String> {
    // 1. Set up the intercom
    let wire = Wire::new(Some(wire_file));
    let soul_side = wire.soul_side();

    let result = crate::wire::with_wire_soul_side(Some(soul_side.clone()), async {
        // 2. Start the mailroom pump
        let pump_handle = self.start_notification_pump(soul_side.clone());

        // 3. Run the turn
        let result = self.run_turn(text).await;

        // 4. Cleanup
        if let Some(handle) = pump_handle {
            handle.abort();
        }
        result
    }).await;

    wire.shutdown().await;
    Ok(result)
}
```

The `run()` method is the **control room's daily routine**. It:

1. Opens the intercom (wire channel) so other rooms can listen
2. Starts the mailroom pump (notification delivery)
3. Runs one **turn** of conversation
4. Closes everything cleanly

🐍 **Python's way:** `run_soul()` is a top-level function that wraps the soul in a wire pump context manager.

🦀 **Rust's way:** Everything is a method on `KimiSoul`. The wire is a local variable dropped at the end of the scope (RAII). No context managers needed — Rust's ownership system _is_ the context manager.

---

## 🎯 One Turn: `run_turn()`

A **turn** is one user message → agent response cycle. Think of it as one round of dialogue.

```rust
// File: octopus-cli/src/soul/kimisoul.rs
async fn run_turn(&mut self, text: &str) -> Result<String> {
    // 1. Parse slash commands
    if let Some(call) = parse_slash_command_call(text) {
        return self._handle_slash_command(call).await;
    }

    // 2. Build the user message
    let user_message = Message { role: "user".to_string(), content: ... };

    // 3. Execute the turn
    let outcome = self.turn(user_message).await?;

    // 4. Return the final text
    Ok(outcome.final_message)
}
```

### Slash Commands: The Emergency Buttons

Slash commands (`/clear`, `/yolo`, `/plan`, etc.) are **control room overrides**. They bypass the LLM entirely and directly manipulate state:

```rust
// soul/slash.rs — 1,240 lines of commands!
registry.register(SlashCommand {
    name: "yolo".to_string(),
    func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
        Box::pin(async move {
            soul.approval.toggle_yolo();
            // ...
        })
    }),
});
```

🐍 **Python's way:** Decorator-based registration `@registry.command("yolo")`.

🦀 **Rust's way:** Imperative registration with `Arc<dyn Fn(...)>` closures. More verbose, but **no proc-macro magic** — you can grep for every registration site.

✨ **Where Rust shines:** **Slash commands are Send + Sync.** The `Arc<dyn Fn(...) -> Pin<Box<dyn Future>>>` type ensures every command can be called from any thread. In Python, the GIL makes this guarantee implicit (and sometimes a bottleneck).

---

## 🧩 One Step: The ReAct Loop

Inside `turn()`, the real magic happens in **steps**. A single turn can have multiple steps if the agent decides to use tools:

```
Step 1: LLM says "I'll search the web"
        → Tool: SearchWeb("Rust vs Python")
Step 2: LLM says "Now I'll read that file"
        → Tool: ReadFile("src/main.rs")
Step 3: LLM says "Here's my answer..."
        → Done!
```

### `step()`: Retry with Grace

```rust
// File: octopus-cli/src/soul/kimisoul.rs
async fn step(&mut self) -> Result<Option<StepOutcome>> {
    for attempt in 1..=max_attempts {
        match self.run_step_once().await {
            Ok(outcome) => return Ok(outcome),
            Err(e) if Self::_is_retryable_error(&e) && attempt < max_attempts => {
                let wait_s = Self::_retry_wait_secs(attempt);
                tokio::time::sleep(Duration::from_secs_f64(wait_s)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

This is **resilience engineering**. If the LLM API flakes, the soul waits and retries with exponential backoff.

### `run_step_once_inner()`: The LLM Call

```rust
// File: octopus-cli/src/soul/kimisoul.rs
let provider = llm.build_kosong_provider()?;
let kosong_history: Vec<kosong::Message> =
    history.iter().map(crate::llm::wire_to_kosong_message).collect();

let mut on_message_part = |part: kosong::StreamedMessagePart| {
    use kosong::chat_provider::Part;
    match part {
        Part::Content(cp) => {
            let wire_cp = crate::llm::kosong_to_wire_content_part(cp);
            wire_send(WireEvent::ContentPart(wire_cp));  // Broadcast to UI in real-time!
        }
        Part::ToolCall(_) | Part::ToolCallPart(_) => {}
    }
};

let step_result = kosong::step_with_callbacks(
    provider.as_ref(),
    &self.agent.system_prompt,
    &KimiToolsetHandle(self.toolset.clone()),
    &kosong_history,
    Some(&mut on_message_part),
    Some(Arc::new(|result: &kosong::ToolResult| {
        wire_send(WireEvent::ToolResult(kosong_to_wire_tool_result(result)));
    })),
)
.await;
```

Notice `kosong::step` — this is the **high-level step abstraction** from the `kosong` crate. It streams LLM output, parses tool calls on the fly, and dispatches them concurrently. The soul doesn't manually juggle `tokio::spawn` handles or `join_all` anymore.

🐍 **Python's way:** Python `kimisoul` calls `kosong.step()` directly. The Python `kosong` library handles streaming and early tool dispatch internally.

🦀 **Rust's way:** We now mirror Python exactly: `kimisoul` delegates to `kosong::step`, then calls `step_result.tool_results().await` to gather finished tools. The manual streaming logic that existed in earlier versions of the Rust rewrite has been replaced by the same abstraction.

✨ **Where Rust shines:** **Construction-invariant safety.** The `on_tool_result` callback is passed to `kosong::step_with_callbacks` at call time, not via a mutable setter. This eliminates a race condition where fast tools could finish before a late-registered callback was installed. If it compiles, the callback is guaranteed to exist before any tool runs.

---

## 🛠️ Tool Execution: Concurrent & Early

When the LLM emits a tool call, `kosong::step_with_callbacks` dispatches it immediately via `KimiToolsetHandle`:

```rust
// Inside kosong::step (kosong/src/step.rs)
let mut on_tool_call = |tool_call: ToolCall| {
    let id = tool_call.id.clone();
    let result = toolset.handle(&tool_call);
    let handle = match result {
        HandleResult::Ready(result) => tokio::spawn(async move { result }),
        HandleResult::Pending(handle) => handle,
    };
    tool_calls.push(tool_call);
    tool_result_futures.insert(id, handle);
};
```

And inside `KimiToolsetHandle::handle`:

```rust
let handle = tokio::spawn(async move {
    let result = inner.handle_inner(&tool_call).await;
    // result is already a kosong::ToolResult — no conversion needed
    result
});
```

Tools are executed **as soon as they're parsed from the stream**, before the LLM even finishes speaking! This is **early tool dispatch** — a major latency win.

🐍 **Python's way:** Python `kosong.step` also dispatches tools eagerly via `asyncio.create_task`. Results are gathered later with `await result.tool_results()`.

🦀 **Rust's way:** `kosong::step_with_callbacks` spawns each tool via `tokio::spawn` the moment its JSON is complete. The `KimiToolsetHandle` bridges `KimiToolset` to kosong's `Toolset` trait. The wire callback lives in `kimisoul.rs`, outside the toolset entirely.

✨ **Where Rust shines:** **Fearless concurrency + type-safe ordering.** The borrow checker ensures `toolset` (an `Arc<KimiToolset>`) is safely shared across tasks. And because the eager callback is passed directly to `step_with_callbacks` — not stored in a mutable field — the compiler guarantees it exists before any task is spawned. In Python, a late `set_on_tool_result` call creates a race window that the language cannot detect.

---

## 🧠 Dynamic Injection: The Reminder System

Before each step, the control room checks for **system reminders**:

```rust
// File: octopus-cli/src/soul/kimisoul.rs
let injections = self.collect_injections().await;
for inj in injections {
    self.context.append_message(Message {
        role: "user".to_string(),
        content: vec![ContentPart::Text {
            text: format!("<system-reminder>\n{}\n</system-reminder>", inj.content),
        }],
    }).await?;
}
```

These injections come from:

- **Plan mode:** "You are in plan mode. Write your plan to the plan file."
- **AFK mode:** "The user is away. Continue autonomously."

🐍 **Python's way:** `collect_injections()` returns a list, injected into the system prompt.

🦀 **Rust's way:** Same pattern, but the `DynamicInjectionProvider` trait lets anyone add new injection sources without modifying the soul:

```rust
// File: octopus-cli/src/soul/dynamic_injection.rs
#[async_trait::async_trait]
pub trait DynamicInjectionProvider: Send + Sync {
    async fn get_injections(
        &mut self,
        history: &[Message],
        ctx: &InjectionContext<'_>,
    ) -> Vec<DynamicInjection>;

    async fn on_context_compacted(&mut self) {}
    async fn on_afk_changed(&mut self, _enabled: bool) {}
}
```

---

## 🔐 Approval Source Tracking: Context Across Async Tasks

Every tool call that needs user approval is tagged with an **approval source** — metadata that says "this request came from turn X" or "this request came from subagent Y." When the turn or subagent ends, the soul cancels any pending approvals that belong to it, preventing stale prompts from lingering.

### Python's Way: `ContextVar`

Python uses `asyncio.ContextVar` — an async-aware global variable that travels with the task across `await` points:

```python
# approval_runtime/runtime.py
_current_approval_source = ContextVar[ApprovalSource | None](
    "current_approval_source", default=None
)

def get_current_approval_source_or_none() -> ApprovalSource | None:
    return _current_approval_source.get()

def set_current_approval_source(source: ApprovalSource) -> Token:
    return _current_approval_source.set(source)

def reset_current_approval_source(token: Token) -> None:
    _current_approval_source.reset(token)
```

Any code — deep inside a tool, a subagent, or a background task — can call `get_current_approval_source_or_none()` and discover what turn or subagent it belongs to.

```python
# soul/kimisoul.py
created_approval_source = None
if get_current_approval_source_or_none() is None:
    created_approval_source = ApprovalSource(kind="foreground_turn", id=uuid.uuid4().hex)
    approval_source_token = set_current_approval_source(created_approval_source)

try:
    # ... run the turn ...
finally:
    if approval_source_token is not None:
        reset_current_approval_source(approval_source_token)
    if created_approval_source is not None:
        self._runtime.approval_runtime.cancel_by_source(
            created_approval_source.kind, created_approval_source.id
        )
```

Key behaviors:

- **Nested turns inherit the parent's source.** A subagent sets its own source; inner turns see it and don't create a new one.
- **Rejection messages are contextual.** If `source.agent_id` is set, the user gets a subagent-specific "try a different approach" message.
- **Cleanup is precise.** Only the source that was _created_ for this scope is cancelled; inherited sources are left alone.

### Rust's Way: `tokio::task_local!`

Rust uses Tokio's task-local storage, which is the async equivalent of a thread-local:

```rust
// File: octopus-cli/src/approval_runtime/runtime.rs
tokio::task_local! {
    static CURRENT_APPROVAL_SOURCE: ApprovalSource;
}

pub fn get_current_approval_source_or_none() -> Option<ApprovalSource> {
    CURRENT_APPROVAL_SOURCE.try_with(|s| s.clone()).ok()
}

pub async fn with_approval_source<T>(
    source: ApprovalSource,
    f: impl std::future::Future<Output = T>,
) -> T {
    CURRENT_APPROVAL_SOURCE.scope(source, f).await
}
```

The `.scope()` call sets the source for the duration of a future and **automatically restores** the previous value when nested. This replaces Python's manual `set` / `reset` token pattern with RAII.

```rust
// File: octopus-cli/src/soul/kimisoul.rs
let (source, inherited) =
    if let Some(existing) = get_current_approval_source_or_none() {
        (existing, true)  // subagent or background task already set one
    } else {
        (ApprovalSource { kind: ApprovalSourceKind::ForegroundTurn, id: uuid::new_v4(), ... }, false)
    };

let result = if inherited {
    self.run_turn_body(text).await
} else {
    with_approval_source(source.clone(), self.run_turn_body(text)).await
};

if !inherited {
    self.approval.runtime().cancel_by_source(&source.kind, &source.id);
}
```

And for subagents:

```rust
// File: octopus-cli/src/tools/agent/mod.rs
let subagent_source = ApprovalSource {
    kind: ApprovalSourceKind::ForegroundTurn,
    id: session.id.clone(),
    agent_id: Some(session.id.clone()),
};
let result = with_approval_source(subagent_source.clone(), subagent.run(prompt)).await;

subagent.approval.runtime().cancel_by_source(subagent_source.kind, &subagent_source.id);
```

✨ **Where Rust shines:** **No manual token bookkeeping.** Python's `try/finally` with `reset_current_approval_source(token)` is error-prone — forget the finally block and the context leaks. Rust's `.scope()` is compile-time safe: the source is automatically restored when the future completes, even if it panics.

| Aspect                     | Python (`ContextVar`)                   | Rust (`tokio::task_local!`)             |
| -------------------------- | --------------------------------------- | --------------------------------------- |
| **Get current**            | `get_current_approval_source_or_none()` | `get_current_approval_source_or_none()` |
| **Set / reset**            | Manual `set()` + `reset(token)`         | `.scope(source, future).await`          |
| **Nested safety**          | Correct if tokens are managed           | Guaranteed by `.scope()` RAII           |
| **Leak on panic**          | Possible if `finally` omitted           | Impossible — scope always restores      |
| **Cross-task inheritance** | Must explicitly copy context            | Same — spawn doesn't inherit            |

---

## 🎁 Souvenir Shop: What to Remember

1. **The soul is a state machine.** `run()` → `run_turn()` → `step()` → `run_step_once()` is a layered hierarchy. Each layer handles one concern.
2. **Streaming is first-class.** The soul doesn't just call the LLM; it _dances_ with it, processing chunks as they arrive and spawning tools mid-stream.
3. **The borrow checker is the safety officer.** Concurrent tool execution, shared state (`Arc<KimiToolset>`), and async lifetimes are all verified at compile time.
4. **Approval source tracking is contextual.** `tokio::task_local!` gives every async task a scoped approval identity. Subagents inherit the parent's source; cleanup is RAII-safe.
5. **~1,290 lines vs. ~1,710 lines.** The Rust soul is 25% smaller than Python's `kimisoul.py` + `__init__.py`, yet includes features (OAuth recovery, notification pump, skill injection) that were scattered across Python's codebase.

---

## 🚶 Next Stop

The Control Room decides _what_ to do. But _how_ to do it? That's the domain of the **Tool Shed** next door.

→ [Tour 3: The Tool Shed](./03-tool-shed.md)
