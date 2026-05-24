# Tour 8: The Archives — Session & Context Management

> *"The basement beneath the basement. Every conversation ever had is stored here, indexed, and ready to be recalled."*

Welcome to the **Archives** — the sub-basement of Octopus-CLI. This is where memory lives:
1. **Sessions** — conversation containers with metadata
2. **Context** — the LLM's short-term memory (message history)
3. **Forking** — copying and truncating conversations

Without the Archives, every chat would start from zero. With them, the agent remembers, resumes, and even travels back in time.

---

## 📁 The Filing System: `Session`

File: `octopus-cli/src/session.rs` (~290 lines)

A `Session` is a **conversation container** — a directory on disk with metadata:

```rust
pub struct Session {
    pub id: String,                    // UUID
    pub work_dir: PathBuf,             // Project root
    pub dir: PathBuf,                  // ~/.kimi/sessions/<id>/
    pub wire_file_path: PathBuf,       // wire.jsonl
    pub context_file_path: PathBuf,    // context.jsonl
    pub state: SessionState,           // title, plan_mode, approval state
}
```

### Creating a Session

```rust
pub async fn create(work_dir: &Path, id: Option<String>) -> io::Result<Session> {
    let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let dir = share::get_share_dir()
        .join("sessions")
        .join(&id);
    tokio::fs::create_dir_all(&dir).await?;

    // Write metadata header
    let meta = json!({
        "version": 1,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "work_dir": work_dir,
    });
    tokio::fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&meta)?).await?;

    Ok(Session { id, work_dir: work_dir.to_path_buf(), dir, ... })
}
```

Every session gets:
- `meta.json` — creation timestamp, work directory
- `wire.jsonl` — event stream (black box recorder)
- `context.jsonl` — message history (LLM's memory)
- `state.json` — session state (title, plan mode, approval)

🐍 **Python's way:** `session.py` uses `pathlib` and `json` module. Similar structure.

🦀 **Rust's way:** `tokio::fs` for async file operations. The session is a plain struct with no methods that surprise you.

✨ **Where Rust shines:** **Async file I/O is explicit.** In Python, `open()` is synchronous (blocks the event loop). You need `aiofiles` for async. In Rust, `tokio::fs` is the standard — every file operation is async by default.

---

## 🧠 The Memory Palace: `Context`

File: `octopus-cli/src/soul/context.rs` (~385 lines)

The `Context` is the LLM's **short-term memory** — a list of messages sent to the API:

```rust
pub struct Context {
    messages: Vec<Message>,
    file_path: Option<PathBuf>,
    checkpoints: Vec<ContextCheckpoint>,
}
```

### Appending Messages

```rust
pub async fn append_message(&mut self, msg: Message) -> io::Result<()> {
    self.messages.push(msg);
    if let Some(ref path) = self.file_path {
        let line = serde_json::to_string(&msg)?;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
    }
    Ok(())
}
```

Every message is **immediately persisted** to `context.jsonl`. If the process crashes mid-conversation, the context is recovered on restart.

### Checkpoints: Time Travel

```rust
pub fn checkpoint(&mut self) -> ContextCheckpoint {
    let checkpoint = ContextCheckpoint {
        message_count: self.messages.len(),
        timestamp: now(),
    };
    self.checkpoints.push(checkpoint.clone());
    checkpoint
}

pub fn revert_to(&mut self, checkpoint: &ContextCheckpoint) {
    self.messages.truncate(checkpoint.message_count);
    // Write truncated context back to disk...
}
```

Checkpoints enable **D-Mail time travel** (from the `BackToTheFuture` error type). If a tool execution corrupts the conversation, the soul can revert to a checkpoint and retry.

🐍 **Python's way:** `context.py` uses list slicing and file rewriting.

🦀 **Rust's way:** `Vec::truncate()` is O(1) — it just moves the length pointer. No memory reallocation.

✨ **Where Rust shines:** **Revert is instant.** `messages.truncate(n)` costs nanoseconds. In Python, `messages = messages[:n]` creates a new list (O(n) copy). For 10,000-message contexts, that's a noticeable pause.

---

## 🍴 The Copier: Session Forking

File: `octopus-cli/src/soul/slash.rs` — `fork_session()` and `enumerate_turns()`

Session forking is **time travel for conversations**. It copies a session's history up to a specific point, creating a new branch.

### Fork at Turn N

```rust
async fn fork_session(
    source: &Session,
    work_dir: &Path,
    turn_index: Option<usize>,
    title_prefix: &str,
) -> io::Result<String> {
    let source_dir = source.dir();
    let wire_src = source_dir.join("wire.jsonl");
    let context_src = source_dir.join("context.jsonl");

    let new_session = Session::create(work_dir, None).await?;
    let new_dir = new_session.dir();

    if wire_src.exists() {
        let content = tokio::fs::read_to_string(&wire_src).await?;
        let lines: Vec<&str> = content.lines().collect();
        let to_write = if let Some(idx) = turn_index {
            lines.into_iter().take(idx + 1).collect::<Vec<_>>().join("\n")
        } else {
            content
        };
        tokio::fs::write(new_dir.join("wire.jsonl"), to_write).await?;
    }
    // ... copy context, set title ...
    Ok(new_session.id)
}
```

🐍 **Python's way:** `session_fork.py` (~325 lines) with complex truncation logic.

🦀 **Rust's way:** ~45 lines. Read file, split lines, take N, write back. The simplicity comes from Rust's iterator chaining.

✨ **Where Rust shines:** **Iterator fusion.** `lines.into_iter().take(idx + 1).collect::<Vec<_>>().join("\n")` is compiled into a single loop with no intermediate allocations. In Python, each method call (`split()`, `[:n]`, `join()`) creates a new list.

### `/undo`: Fork and Switch

```rust
// /undo <turn_number>
let turns = enumerate_turns(&soul.session.wire_file_path);
let turn_idx = if args.trim().is_empty() {
    turns.len().saturating_sub(1)  // Default: last turn
} else {
    args.trim().parse::<usize>()? - 1
};

let new_id = fork_session(&soul.session, &work_dir, Some(turn_idx), "Undo").await?;
// Switch to new session and exit current one
```

`/undo` is the **"I regret this conversation"** button. It forks at a previous turn and switches to the new session.

---

## 📊 Compaction: When Memory Gets Full

When the context approaches the LLM's token limit, the soul **compacts** it:

```rust
pub async fn compact_context(&mut self, custom_instruction: &str) -> Result<()> {
    let llm = self.llm.as_ref().ok_or(LLMNotSet::NotSet)?;
    let history = self.context.history().to_vec();

    let summary = llm.complete(
        Some("Summarize this conversation for future reference."),
        &history,
        None,
    ).await?;

    self.context.clear()?;
    self.context.append_message(Message {
        role: "system".to_string(),
        content: vec![ContentPart::Text {
            text: format!("Previous conversation summary:\n{}", summary),
        }],
    }).await?;
}
```

Compaction is **lossy compression**: the full history is replaced by an LLM-generated summary. This frees up tokens for new conversation.

🐍 **Python's way:** Similar, with Jinja2 templating for the compaction prompt.

🦀 **Rust's way:** Direct string formatting. The compaction prompt is embedded in `prompts/compact.md`.

---

## 🎁 Souvenir Shop: What to Remember

1. **Sessions are directories.** Every conversation is a self-contained folder. You can `ls ~/.kimi/sessions/` and see your entire chat history.
2. **Context is append-only.** `context.jsonl` grows monotonically. Reverts truncate in-memory; the file is rewritten on save.
3. **Forking is cheap.** It's just `read → split → take → write`. No database, no complex state management.
4. **Compaction is an LLM call.** The soul asks itself to summarize itself. Meta!

---

## 🚶 Next Stop

The Archives preserve the past. But who watches the watchers? Let's ascend to the **Observatory** — where telemetry and hooks monitor everything.

→ [Tour 9: The Observatory](./09-observatory.md)
