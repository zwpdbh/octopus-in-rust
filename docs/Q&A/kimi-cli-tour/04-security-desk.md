# Tour 4: The Security Desk — Approval & OAuth

> *"Every powerful building needs security. This desk decides who gets in, what tools can be used, and when to ask for permission."*

Welcome to the **Security Desk** — the first floor, where two systems guard the building:
1. **Approval Flow** — "Should the agent run this tool?"
2. **OAuth** — "Who is this user, and are they authenticated?"

These systems are conceptually separate but practically intertwined: the approval system protects the user from the agent, while OAuth protects the agent's access to external APIs.

---

## 🛡️ Part 1: The Approval System

### The Actors

Three entities participate in every approval decision:

| Actor | File | Role |
|-------|------|------|
| `ApprovalRuntime` | `approval_runtime/mod.rs` | The **judge** — stores requests, waits for resolution |
| `Approval` (wrapper) | `soul/approval.rs` | The **clerk** — checks yolo/afk state, routes to runtime |
| `ShellUI` | `ui/shell/mod.rs` | The **jury** — renders the prompt, collects user input |

### The Flow

```
Tool wants to run
    → Approval::request() checks yolo/afk
        → If yolo: auto-approve
        → If afk: auto-approve (user is away)
        → Otherwise: create ApprovalRequest
            → ApprovalRuntime publishes to RootWireHub
                → ShellUI receives event, renders overlay
                    → User presses Y/N/A
                        → ApprovalRuntime resolves via oneshot channel
                            → Tool runs (or is rejected)
```

### The Judge: `ApprovalRuntime`

```rust
pub struct ApprovalRuntime {
    inner: Arc<Mutex<ApprovalRuntimeInner>>,
    hub: Option<RootWireHub>,
}

struct ApprovalRuntimeInner {
    requests: HashMap<String, ApprovalRequestRecord>,
    waiters: HashMap<String, tokio::sync::oneshot::Sender<ApprovalResponse>>,
}
```

The `ApprovalRuntime` is a **state machine** with two hashmaps:
- `requests`: pending approval requests
- `waiters`: oneshot channels for async waiters

🐍 **Python's way:** `asyncio.Event` and `asyncio.Queue` for synchronization. The runtime lives in the main event loop.

🦀 **Rust's way:** `tokio::sync::oneshot` channels. When `wait_for_response()` is called, it creates a oneshot receiver and stores the sender in `waiters`. When `resolve()` is called, it sends the response through the channel.

✨ **Where Rust shines:** **No polling.** The waiter is suspended by the Tokio runtime until the oneshot fires. No CPU cycles wasted on `while not resolved: sleep(0.1)`. In Python, `asyncio.Event.wait()` is efficient too, but the oneshot pattern is more explicit about "exactly one response."

### The Overlay

In the TUI, approval requests appear as a modal overlay:

```rust
fn render_approval_overlay(&self, frame: &mut Frame, req: &ApprovalRequestEvent) {
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Approve ", Style::default().fg(Color::White)),
            Span::styled(&req.action, Style::default().fg(Color::Yellow)),
            Span::styled("?", Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from("[Y]es  [N]o  [A]lways"),
    ];
    // Render centered overlay...
}
```

The overlay **pauses the entire TUI** until resolved. This is critical — you don't want the agent running tools while you're still reading the prompt!

---

## 🔐 Part 2: OAuth Authentication

### The Vault: `OAuthManager`

File: `octopus-cli/src/auth/mod.rs` (~251 lines)

The `OAuthManager` handles token storage, refresh, and API key resolution:

```rust
pub struct OAuthManager {
    access_tokens: HashMap<String, String>,
    refresh_lock: tokio::sync::Mutex<()>,
    rejected_refresh_tokens: HashMap<String, (String, Instant)>,
}
```

### Token Storage: Atomic & Permission-Safe

```rust
pub fn save_tokens(key: &str, token: &OAuthToken) -> Result<()> {
    let path = token_path(key);
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir)?;

    let mut file = OpenOptions::new()
        .mode(0o600)  // Owner-only permissions!
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)?;

    let json = serde_json::to_string_pretty(token)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;  // Ensure data hits disk
    Ok(())
}
```

🐍 **Python's way:** `json.dump()` to a file, then `os.chmod(path, 0o600)`.

🦀 **Rust's way:** Single syscall with atomic permissions. No race window.

✨ **Where Rust shines:** **The `0o600` is set at creation time.** In Python, a malicious process could read the file between `open()` and `chmod()`. In Rust, the file is born secure.

### Token Refresh: Retry with Tombstones

When the LLM API returns 401, the soul triggers token refresh:

```rust
pub async fn ensure_fresh(&self, llm: &LLM, force: bool) -> Result<Option<String>> {
    let token = load_tokens("kimi-code");
    let expires_at = token.expires_at;

    let threshold = max(300.0, 0.5 * token.expires_in);
    if !force && now < expires_at - threshold {
        return Ok(None);  // Token is fresh enough
    }

    // Check tombstone (rejected refresh token)
    if let Some((_, cooldown_until)) = self.rejected_refresh_tokens.get(refresh_token) {
        if now < *cooldown_until {
            return Ok(None);  // Don't retry a known-bad token
        }
    }

    let _guard = self.refresh_lock.lock().await;  // Only one refresh at a time!
    match refresh_token(refresh_token).await {
        Ok(new_token) => {
            save_tokens("kimi-code", &new_token)?;
            Ok(Some(new_token.access_token))
        }
        Err(e) => {
            // Tombstone for 5 minutes
            self.rejected_refresh_tokens.insert(
                refresh_token.to_string(),
                (refresh_token.to_string(), now + 300.0),
            );
            Err(e)
        }
    }
}
```

This is **resilient token management**:
1. **Proactive refresh** — refresh before expiry, not after
2. **Tombstones** — if a refresh token is rejected, don't retry it for 5 minutes
3. **Mutex lock** — only one refresh happens at a time, even with concurrent requests
4. **Threshold heuristic** — `max(300s, 0.5 * expires_in)` balances safety and API load

🐍 **Python's way:** Similar logic, but tombstones and locking are handled by `threading.Lock()` and `time.sleep()`.

🦀 **Rust's way:** `tokio::sync::Mutex` for async locking. The tombstone `HashMap` is owned by `OAuthManager` and protected by the struct's `&mut self` borrow.

✨ **Where Rust shines:** **The refresh lock is composable.** Because `tokio::sync::Mutex::lock()` returns a future, you can `.await` it inside any async function. The compiler ensures you can't forget to release the lock (the guard implements `Drop`). In Python, a `with lock:` block is similar, but an unhandled exception could leak the lock state.

---

## 🎭 The Yolo/Afk Modes

The Security Desk has three modes of operation:

| Mode | Trigger | Behavior |
|------|---------|----------|
| **Normal** | Default | Every tool asks for approval |
| **YOLO** | `/yolo` | Auto-approve everything this session |
| **AFK** | `/afk` | Auto-approve; user is away |

```rust
pub struct ApprovalState {
    pub yolo: bool,
    pub afk: bool,
    pub auto_approve: Vec<String>,  // Per-tool auto-approve list
}
```

These states are **persisted to the session** and synced at the end of every turn:

```rust
async fn _sync_approval_state(&self) {
    let state = self.approval.state();
    self.session.state.approval_yolo = state.yolo;
    self.session.state.approval_afk = state.afk;
    let _ = self.session.save_state().await;
}
```

This means if you type `/yolo`, quit, and resume tomorrow — **YOLO is still active**. The building remembers your security preferences.

---

## 🎁 Souvenir Shop: What to Remember

1. **Approval is a distributed system.** The runtime, the wrapper, and the UI are decoupled by the `RootWireHub` broadcast channel. Any UI (TUI, web, IDE) can subscribe and render approvals.
2. **OAuth is defense in depth.** Atomic file creation, tombstone cooldowns, mutex-locked refreshes, and proactive expiry checking — each layer protects against a different failure mode.
3. **YOLO/AFK are session-persistent.** The security desk remembers your preferences across restarts.
4. **No global state.** `OAuthManager` and `ApprovalRuntime` are owned by `KimiSoul`. You could run two souls with different OAuth credentials or approval settings in the same process.

---

## 🚶 Next Stop

The Security Desk guards the building. But how do the rooms communicate with each other? Let's visit the **Communication Hub**.

→ [Tour 5: The Communication Hub](./05-communication-hub.md)
