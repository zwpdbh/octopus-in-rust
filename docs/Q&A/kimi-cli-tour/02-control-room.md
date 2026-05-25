# Tour 2: The Control Room — The Soul of the Machine

> *"This is where the building thinks. Every decision, every word, every tool call originates here."*

Welcome to the **Control Room** — the second floor, and the beating heart of Octopus-CLI. This is where `KimiSoul` lives. If the lobby is the building's entrance, the Control Room is its **brain**.

In this room, we'll watch the agent **observe** (read user input), **think** (call the LLM), **act** (execute tools), and **remember** (persist context). This cycle — the **ReAct loop** — is the fundamental pattern of modern AI agents.

---

## 🧠 The Central Computer: `KimiSoul`

File: `octopus-cli/src/soul/kimisoul.rs` (~1,290 lines)

The `KimiSoul` struct is the control room's main console. Every field is a piece of equipment:

```rust
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
pub async fn run(&mut self, user_input: &str) -> Result<String> {
    // 1. Set up the intercom
    let wire = Wire::new(Some(wire_file));
    crate::wire::set_current_wire_soul_side(Some(wire.soul_side()));

    // 2. Start the mailroom pump
    let pump_handle = self._start_notification_pump(...);

    // 3. Run the turn
    let result = self._run_turn(text).await;

    // 4. Cleanup
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

🦀 **Rust's way:** Everything is a method on `KimiSoul`. The wire is a local variable dropped at the end of the scope (RAII). No context managers needed — Rust's ownership system *is* the context manager.

---

## 🎯 One Turn: `_run_turn()`

A **turn** is one user message → agent response cycle. Think of it as one round of dialogue.

```rust
async fn _run_turn(&mut self, text: &str) -> Result<String> {
    // 1. Parse slash commands
    if let Some(call) = parse_slash_command_call(text) {
        return self._handle_slash_command(call).await;
    }

    // 2. Build the user message
    let user_message = Message { role: "user".to_string(), content: ... };

    // 3. Execute the turn
    let outcome = self._turn(user_message).await?;

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
            soul.approval.state_mut().yolo = true;
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

Inside `_turn()`, the real magic happens in **steps**. A single turn can have multiple steps if the agent decides to use tools:

```
Step 1: LLM says "I'll search the web"
        → Tool: SearchWeb("Rust vs Python")
Step 2: LLM says "Now I'll read that file"
        → Tool: ReadFile("src/main.rs")
Step 3: LLM says "Here's my answer..."
        → Done!
```

### `_step()`: Retry with Grace

```rust
async fn _step(&mut self) -> Result<Option<StepOutcome>> {
    for attempt in 1..=max_attempts {
        match self._run_step_once().await {
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

### `_run_step_once_inner()`: The LLM Call

```rust
let tools_slice: Vec<&dyn Tool> = self.toolset.tools();
let system_prompt = self.agent.as_ref().map(|a| a.system_prompt.as_str());

let completion_result = llm
    .generate_streaming(
        system_prompt,
        &history,
        Some(&tools_slice),
        &mut on_message_part,
        &mut on_tool_call,
    )
    .await;
```

Notice `generate_streaming` — this is **streaming LLM output**. The soul doesn't wait for the full response; it processes chunks as they arrive:

```rust
let mut on_message_part = |part: kosong::StreamedMessagePart| {
    match part {
        Part::Content(cp) => {
            let wire_cp = crate::llm::kosong_to_wire_content_part(cp);
            wire_send(wire_cp);  // Broadcast to UI in real-time!
        }
        Part::ToolCall(_) | Part::ToolCallPart(_) => {}
    }
};
```

🐍 **Python's way:** Async generators (`yield`) stream content. The UI polls or listens via asyncio queues.

🦀 **Rust's way:** Closure-based streaming. The `on_message_part` closure is called **synchronously** for each chunk, inside the LLM provider's stream loop. No queues, no polling — direct callback.

✨ **Where Rust shines:** **Zero-allocation streaming.** The closure receives a borrowed `StreamedMessagePart`. No `Arc<Mutex<Vec<Chunk>>>` buffer, no channel send/recv overhead. The bytes flow from the HTTP response directly into the wire channel with minimal copying.

---

## 🛠️ Tool Execution: Concurrent & Early

When the LLM emits a tool call, something clever happens:

```rust
let mut on_tool_call = move |tc: kosong::ToolCall| {
    let toolset = toolset.clone();
    let handle = tokio::spawn(async move {
        toolset.handle(&wire_tc).await
    });
    handles.lock().unwrap().push(handle);
};
```

Tools are executed **as soon as they're parsed from the stream**, before the LLM even finishes speaking! This is **early tool dispatch** — a major latency win.

🐍 **Python's way:** Wait for the full LLM response, parse tool calls from the complete message, then execute them sequentially.

🦀 **Rust's way:** Spawn each tool in a `tokio::task` the moment its JSON is complete. Tools run **concurrently** while the LLM keeps generating text.

✨ **Where Rust shines:** **Fearless concurrency.** The borrow checker ensures `toolset` (an `Arc<KimiToolset>`) is safely shared across tasks. In Python, you'd need careful `asyncio` orchestration to avoid race conditions. In Rust, if it compiles, it's race-free.

---

## 🧠 Dynamic Injection: The Reminder System

Before each step, the control room checks for **system reminders**:

```rust
let injections = self._collect_injections().await;
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

🐍 **Python's way:** `_collect_injections()` returns a list, injected into the system prompt.

🦀 **Rust's way:** Same pattern, but the `DynamicInjectionProvider` trait lets anyone add new injection sources without modifying the soul:

```rust
pub trait DynamicInjectionProvider {
    fn inject(&self, soul: &KimiSoul) -> Vec<DynamicInjection>;
}
```

---

## 🎁 Souvenir Shop: What to Remember

1. **The soul is a state machine.** `run()` → `_run_turn()` → `_step()` → `_run_step_once()` is a layered hierarchy. Each layer handles one concern.
2. **Streaming is first-class.** The soul doesn't just call the LLM; it *dances* with it, processing chunks as they arrive and spawning tools mid-stream.
3. **The borrow checker is the safety officer.** Concurrent tool execution, shared state (`Arc<KimiToolset>`), and async lifetimes are all verified at compile time.
4. **~1,290 lines vs. ~1,710 lines.** The Rust soul is 25% smaller than Python's `kimisoul.py` + `__init__.py`, yet includes features (OAuth recovery, notification pump, skill injection) that were scattered across Python's codebase.

---

## 🚶 Next Stop

The Control Room decides *what* to do. But *how* to do it? That's the domain of the **Tool Shed** next door.

→ [Tour 3: The Tool Shed](./03-tool-shed.md)
