# Tour 6: The Workshop — Background Tasks & Subagents

> *"The basement is noisy. Sparks fly. Processes spawn. This is where the building does its heavy lifting while the upstairs guests enjoy tea."*

Welcome to the **Workshop** — the basement of Octopus-CLI. This is where long-running work happens **outside** the main conversation flow:
1. **Background Tasks** — shell commands that outlive the current turn
2. **Subagents** — secondary AI agents spawned for parallel work

These systems share a philosophy: **the main agent shouldn't wait.**

---

## 🔧 Part 1: Background Tasks

File: `octopus-cli/src/background/mod.rs` (~205 lines)

The `BackgroundTaskManager` is the workshop foreman. It tracks running processes and their output.

### The Data Structure

```rust
#[derive(Clone)]
pub struct BackgroundTaskManager {
    tasks: Arc<std::sync::Mutex<HashMap<String, TaskHandle>>>,
}

struct TaskHandle {
    task: BackgroundTask,
    child: Arc<Mutex<Child>>,          // The process
    output: Arc<Mutex<String>>,        // Captured stdout+stderr
}

#[derive(Debug, Clone)]
pub struct BackgroundTask {
    pub id: String,
    pub command: String,
    pub description: String,
    pub status: TaskStatus,
    pub output: String,
}
```

Notice the **three layers of Arc/Mutex**:
1. `Arc<std::sync::Mutex<HashMap<...>>>` — shared task registry
2. `Arc<Mutex<Child>>` — shared process handle
3. `Arc<Mutex<String>>` — shared output buffer

🐍 **Python's way:** `asyncio.Queue` + `asyncio.Lock` for shared state. Processes managed by `asyncio.create_subprocess_exec()`.

🦀 **Rust's way:** `Arc` for shared ownership, `tokio::sync::Mutex` for async-safe mutation, `std::sync::Mutex` for sync-safe mutation (the HashMap is only accessed briefly).

✨ **Where Rust shines:** **Mixed locking strategy.** The HashMap uses `std::sync::Mutex` (cheaper, no async overhead) because lock contention is brief. The Child and output use `tokio::sync::Mutex` because they're held across `.await` points. In Python, you'd use `asyncio.Lock` for everything — more uniform but less efficient.

### Spawning a Task

```rust
pub async fn spawn(&self, command: String, description: String) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();

    let mut child = tokio::process::Command::new("bash")
        .arg("-c")
        .arg(&command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn: {}", e))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let child = Arc::new(Mutex::new(child));
    let output = Arc::new(Mutex::new(String::new()));

    // Spawn reader tasks
    let output_clone = output.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let mut out = output_clone.lock().await;
            out.push_str(&line);
            out.push('\n');
        }
    });

    // Store in registry
    self.tasks.lock().unwrap().insert(id.clone(), TaskHandle { ... });
    Ok(id)
}
```

When a task spawns:
1. A `tokio::process::Child` is created
2. **Two reader tasks** are spawned — one for stdout, one for stderr
3. Both append to a shared `Arc<Mutex<String>>`
4. The task handle is stored in the registry

🐍 **Python's way:** Similar, but Python's `asyncio.subprocess` has higher overhead per process.

🦀 **Rust's way:** `tokio::process::Child` is a thin wrapper around OS process handles. The reader tasks are green threads (cooperative scheduling), so 1,000 background tasks consume ~1MB of memory, not 50GB.

✨ **Where Rust shines:** **Tasks are cheaper than threads.** Each background reader is a `tokio::spawn` future — ~200 bytes of memory. In Python, each subprocess reader might need a thread (~8MB stack). Rust's async runtime handles I/O multiplexing for thousands of tasks on a single OS thread pool.

### Killing a Task

```rust
pub async fn stop(&self, id: &str) -> Result<(), String> {
    let child_arc = {
        let tasks = self.tasks.lock().unwrap();
        let handle = tasks.get(id).ok_or("Task not found")?;
        handle.child.clone()
    };  // Lock dropped here!

    let mut child = child_arc.lock().await;
    child.kill().await.map_err(|e| format!("Failed to kill: {}", e))?;

    let mut tasks = self.tasks.lock().unwrap();
    if let Some(handle) = tasks.get_mut(id) {
        handle.task.status = TaskStatus::Killed;
    }
    Ok(())
}
```

Notice the **careful lock discipline**:
1. Acquire `tasks` lock, clone the `Arc<Child>`, drop lock
2. `.await` on `child.kill()` — this might take time!
3. Re-acquire `tasks` lock to update status

This pattern avoids holding a sync mutex across an `.await` point, which would deadlock.

---

## 🧬 Part 2: Subagents

File: `octopus-cli/src/tools/agent/mod.rs` (~160 lines)

A **subagent** is a recursive agent call — a second `KimiSoul` spawned to handle a sub-task.

### The Spawner

```rust
async fn run_subagent(
    config: Config,
    llm: Option<LLM>,
    approval_state: ApprovalState,
    work_dir: PathBuf,
    prompt: &str,
) -> Result<String, String> {
    let session = Session::create(&work_dir, None)
        .await
        .map_err(|e| format!("Failed to create session: {}", e))?;

    let mut soul = KimiSoul::new(config, session, llm, approval_state);
    soul.run(prompt).await
        .map_err(|e| format!("Subagent failed: {}", e))
}
```

This is **agentic recursion**. The parent agent creates a child agent with:
- **Fresh session** — isolated context, no pollution of parent's conversation
- **Same config** — same model, same tools, same rules
- **Same approval** — child must also ask for permission

### Foreground vs. Background

```rust
// Foreground: parent waits
let result = run_subagent(config, llm, approval, work_dir, prompt).await?;

// Background: parent continues
let handle = tokio::spawn(async move {
    let result = run_subagent(config, llm, approval, work_dir, prompt).await;
    tracing::info!("Background subagent completed: {}", result);
    // TODO: send notification to parent
});
```

🐍 **Python's way:** Subagents are managed by `SubagentStore` with registry, builder pattern, and output formatting.

🦀 **Rust's way:** A simple function call. No registry, no builder — just create a soul and run it.

✨ **Where Rust shines:** **Recursive async is safe.** Each `KimiSoul::run()` is a future compiled into a state machine. The stack doesn't grow. You can nest subagents 10 levels deep without stack overflow. In Python, deep recursion risks `RecursionError` or C stack exhaustion.

### The Missing Piece: Parent Notification

There's a `// TODO: send notification to parent` in the background subagent code. When a background subagent finishes, the parent should be notified via the wire or notification system. This is a P2 gap — the core functionality works, but the "your subagent is done" UX is missing.

---

## 🔌 Integration: How the Workshop Connects

Background tasks and subagents are triggered by tools:

```rust
// ShellTool spawns background tasks
ShellTool::call({ "command": "cargo build", "run_in_background": true })
    → BackgroundTaskManager::spawn()

// AgentTool spawns subagents
AgentTool::call({ "prompt": "Refactor this module" })
    → run_subagent() → new KimiSoul::run()
```

Both systems feed **back into the main agent** through:
- **Wire events** — real-time status updates
- **Notifications** — persistent delivery to LLM context
- **Tool results** — `TaskOutputTool` reads background output

---

## 🎁 Souvenir Shop: What to Remember

1. **Background tasks are OS processes + Tokio readers.** The process does the work; Tokio tasks capture output concurrently.
2. **Subagents are recursive souls.** Each subagent is a full `KimiSoul` with fresh memory. This is powerful but resource-intensive — don't spawn 100 subagents.
3. **Lock discipline matters.** Never hold a `std::sync::Mutex` across `.await`. Clone the `Arc`, drop the lock, then await.
4. **Tasks are lightweight, processes are heavy.** 1,000 Tokio tasks = ~1MB. 1,000 OS processes = ~50GB. The workshop uses processes for isolation (shell commands) and tasks for I/O (readers).

---

## 🚶 Next Stop

The Workshop handles heavy lifting in the basement. Now let's ascend to the **Front Desk** — where users actually interact with the building.

→ [Tour 7: The Front Desk](./07-front-desk.md)
