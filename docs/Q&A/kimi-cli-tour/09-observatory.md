# Tour 9: The Observatory — Telemetry & Hooks

> *"From the rooftop, you can see everything. Every tool call, every error, every user action — all tracked, measured, and beamed to the stars."*

Welcome to the **Observatory** — the rooftop of Octopus-CLI. This floor houses two surveillance systems:
1. **Telemetry** — anonymous event tracking for product improvement
2. **Hooks** — user-defined scripts that run before/after tools, on compaction, etc.

These systems are **optional but powerful**. They let users observe and customize the agent's behavior without modifying core code.

---

## 📡 Part 1: Telemetry

Files: `octopus-cli/src/telemetry/` — `mod.rs`, `sink.rs`, `transport.rs`

Telemetry answers the question: **"How is the CLI being used?"** Events include tool calls, API errors, compaction triggers, and turn interruptions.

### The `track!` Macro

```rust
// Example: telemetry macro usage
// Anywhere in the codebase:
crate::track!(
    "tool_call",
    tool_name = "ReadFile",
    outcome = "success",
    duration_ms = 42,
);
```

This macro expands to:
1. Build a JSON event: `{"event": "tool_call", "tool_name": "ReadFile", ...}`
2. Enrich with context (app version, platform, model name)
3. Buffer in a global queue
4. Flush to the HTTP sink periodically

🐍 **Python's way:** Function calls to a telemetry client with dict payloads.

🦀 **Rust's way:** A macro with named arguments. The compiler validates syntax at compile time.

✨ **Where Rust shines:** **Zero-cost when disabled.** If telemetry is never initialized, the `track!` macro is a no-op. The compiler optimizes it away entirely. In Python, every `track()` call still builds a dict and makes a function call.

### The Transport Layer

```rust
// File: octopus-cli/src/telemetry/transport.rs
pub struct AsyncTransport {
    endpoint: String,
    client: reqwest::Client,
}

impl AsyncTransport {
    pub async fn send_batch(&self, events: Vec<Map<String, Value>>) -> Result<()> {
        let response = self.client.post(&self.endpoint)
            .json(&events)
            .send()
            .await?;

        match response.status() {
            status if status.is_server_error() || status == 429 => {
                // Retry with exponential backoff: 1s, 4s, 16s
                Err(Error::Retryable)
            }
            status if status.is_client_error() => {
                // Drop non-retryable events
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
```

The transport has **three retry policies**:
1. **5xx / 429:** Exponential backoff (`[1s, 4s, 16s]`)
2. **4xx:** Drop the event (don't retry a bad request forever)
3. **401:** Retry anonymously without auth token

### Disk Fallback

If HTTP fails after all retries, events are **spooled to disk**:

```rust
// File: octopus-cli/src/telemetry/transport.rs
let fallback_path = get_telemetry_dir().join(format!("failed_{}.jsonl", uuid));
let mut file = std::fs::File::create(&fallback_path)?;
for event in events {
    serde_json::to_writer(&mut file, &event)?;
    file.write_all(b"\n")?;
}
```

On startup, the telemetry system **scans for failed events** and retries them:

```rust
// File: octopus-cli/src/telemetry/sink.rs
pub fn retry_disk_events(&self) -> Vec<Map<String, Value>> {
    let mut recovered = Vec::new();
    for entry in std::fs::read_dir(get_telemetry_dir()).ok()? {
        let path = entry?.path();
        if path.file_name()?.to_str()?.starts_with("failed_") {
            let content = std::fs::read_to_string(&path)?;
            for line in content.lines() {
                recovered.push(serde_json::from_str(line)?);
            }
            let _ = std::fs::remove_file(&path);  // Clean up after recovery
        }
    }
    recovered
}
```

🐍 **Python's way:** SQLite or file-based queue with `aiohttp` retry logic.

🦀 **Rust's way:** Simple JSONL files with TTL-based cleanup. No database dependency.

✨ **Where Rust shines:** **Startup recovery is synchronous.** The `retry_disk_events()` function runs in `main()` before the async runtime starts. This means even if the previous session crashed, we recover telemetry before anything else happens.

---

## 🪝 Part 2: Hooks

Files: `octopus-cli/src/hooks/` — `mod.rs`, `runner.rs`, `events.rs`

Hooks let users **intercept and customize** the agent's behavior. They're shell scripts (or any executable) that run at specific events.

### Configuring Hooks

In `~/.kimi/config.toml`:

```toml
[[hooks]]
event = "PreToolUse"
pattern = "write_file|str_replace_file"
command = "./scripts/ask_before_write.sh"
timeout_ms = 5000
```

This hook runs **before any write operation**, giving the user a chance to block it.

### The Hook Engine

```rust
// File: octopus-cli/src/hooks/engine.rs
pub struct HookEngine {
    hooks: Vec<ConfiguredHook>,
    event_index: HashMap<String, Vec<usize>>,  // event → hook indices
}

impl HookEngine {
    pub async fn evaluate(&self, event: &str, payload: &str) -> Vec<HookResult> {
        let indices = self.event_index.get(event).unwrap_or(&vec![]);
        let futures: Vec<_> = indices.iter()
            .map(|&idx| self.run_hook(&self.hooks[idx], payload))
            .collect();
        futures::future::join_all(futures).await
    }
}
```

Hooks matching the same event run **in parallel** via `join_all`. Each hook gets the event payload as JSON on stdin and returns a result on stdout.

### Hook Actions

A hook can return:
- **`{"action": "allow"}`** — proceed normally
- **`{"action": "block", "reason": "..."}`** — abort the tool call

```rust
// File: octopus-cli/src/hooks/runner.rs
pub enum HookAction {
    Allow,
    Block { reason: String },
}
```

🐍 **Python's way:** `HookEngine` in `hooks/engine.py` with regex matching and `asyncio.gather()`.

🦀 **Rust's way:** Same architecture, but the regex engine and subprocess runner are native Rust. No Python interpreter overhead per hook invocation.

✨ **Where Rust shines:** **Hook subprocesses are pure OS processes.** They don't need a Python runtime. You can write hooks in Bash, Go, Rust, or anything that reads stdin and writes stdout. In Python's kimi-cli, hooks also run as subprocesses, but the parent process is heavier.

### Event Types

| Event | When It Fires |
|-------|---------------|
| `PreToolUse` | Before a tool executes |
| `PostToolUse` | After a tool succeeds |
| `PostToolUseFailure` | After a tool fails |
| `UserPromptSubmit` | When the user submits input |
| `Stop` | When a turn completes normally |
| `StopFailure` | When a turn fails |
| `PreCompact` | Before context compaction |
| `PostCompact` | After context compaction |
| `Notification` | When a notification is delivered |

---

## 🔗 Integration: How Telemetry and Hooks Work Together

```
User submits prompt
    → Hook: UserPromptSubmit
        → KimiSoul runs turn
            → Hook: PreToolUse
                → Tool executes
                    → Hook: PostToolUse
                        → Telemetry: tool_call event
            → Turn completes
                → Hook: Stop
                    → Telemetry: turn_completed event
```

Hooks are **synchronous gates** (they can block actions). Telemetry is **asynchronous fire-and-forget** (it never blocks).

---

## 🎁 Souvenir Shop: What to Remember

1. **Telemetry is opt-out by design.** Events are anonymous (no user IDs, no conversation content). The `track!` macro is a no-op until a sink is attached.
2. **Hooks are user-defined middleware.** They turn the agent from a black box into a customizable pipeline.
3. **Disk fallback makes telemetry resilient.** Even without network, events are preserved and retried later.
4. **Hook evaluation is parallel.** Multiple hooks on the same event run concurrently, not sequentially.

---

## 🏁 End of Tour

Congratulations! You've visited every floor of Octopus-CLI:

| Floor | What You Saw |
|-------|-------------|
| 🚪 [Lobby](./01-lobby.md) | CLI parsing, app lifecycle, OAuth |
| 🧠 [Control Room](./02-control-room.md) | `KimiSoul`, agent loop, ReAct pattern |
| 🔧 [Tool Shed](./03-tool-shed.md) | `Tool` trait, execution pipeline, MCP |
| 🛡️ [Security Desk](./04-security-desk.md) | Approval flow, Y/N/A prompts, token refresh |
| 📡 [Communication Hub](./05-communication-hub.md) | Wire protocol, notifications, broadcast |
| 🔩 [Workshop](./06-workshop.md) | Background tasks, subagent recursion |
| 🖥️ [Front Desk](./07-front-desk.md) | TUI shell, markdown rendering, clipboard |
| 📁 [Archives](./08-archives.md) | Sessions, context, forking, compaction |
| 🔭 **Observatory** | Telemetry, hooks, event tracking |

### The Building in Numbers

| | Python (kimi-cli) | Rust (octopus-cli) |
|---|-------------------|--------------------|
| Total LOC | ~47,200 | ~15,200 (32%) |
| Core agent loop | ~1,714 lines | ~1,290 lines |
| TUI shell | ~4,032 lines | ~1,148 lines |
| Tool system | ~1,200 lines | ~1,100 lines |
| Files | ~120 `.py` | ~75 `.rs` |

The Rust rewrite achieves **feature parity** with **one-third the code** — not by cutting corners, but by leveraging Rust's type system, zero-cost abstractions, and fearless concurrency.

Thank you for visiting. The building is open source. Feel free to move in. 🦀
