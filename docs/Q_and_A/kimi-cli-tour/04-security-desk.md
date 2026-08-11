# Tour 4: The Security Desk — Approval & OAuth

> _"Every powerful building needs security. This desk decides who gets in, what tools can be used, and when to ask for permission."_

Welcome to the **Security Desk** — the first floor, where two systems guard the building:

1. **Approval Flow** — "Should the agent run this tool?"
2. **OAuth** — "Who is this user, and are they authenticated?"

These systems are conceptually separate but practically intertwined: the approval system protects the user from the agent, while OAuth protects the agent's access to external APIs.

---

## 🛡️ Part 1: The Approval System

### The Actors

Three entities participate in every approval decision:

| Actor                | File                          | Role                                                     |
| -------------------- | ----------------------------- | -------------------------------------------------------- |
| `ApprovalRuntime`    | `approval_runtime/runtime.rs` | The **judge** — stores requests, waits for resolution    |
| `Approval` (wrapper) | `soul/approval.rs`            | The **clerk** — checks yolo/afk state, routes to runtime |
| `ShellUI`            | `ui/shell/mod.rs`             | The **jury** — renders the prompt, collects user input   |

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
                            → Y: `Allow { scope: Once }`
                            → A: `Allow { scope: Session }`
                            → N: `Reject { feedback }`
                            → Tool runs (or is rejected)
```

### The Judge: `ApprovalRuntime`

```rust
// File: octopus-cli/src/approval_runtime/runtime.rs
pub struct ApprovalRuntime {
    inner: Arc<Mutex<ApprovalRuntimeInner>>,
}

#[derive(Debug, Default)]
struct ApprovalRuntimeInner {
    requests: HashMap<String, ApprovalRequest>,
    waiters: HashMap<String, oneshot::Sender<ApprovalResponse>>,
    hub: Option<RootWireHub>,
}

pub enum ApprovalScope {
    Once,    // Approve this action only
    Session, // Approve and remember for this session
}

pub enum ApprovalResponse {
    Allow { scope: ApprovalScope },
    Reject { feedback: String },
}
```

The `ApprovalRuntimeInner` is a **state machine** with three fields:

- `requests`: pending approval requests
- `waiters`: oneshot channels for async waiters
- `hub`: optional `RootWireHub` for broadcasting approval events to the UI

🐍 **Python's way:** `asyncio.Event` and `asyncio.Queue` for synchronization. The runtime lives in the main event loop.

🦀 **Rust's way:** `tokio::sync::oneshot` channels. When `wait_for_response()` is called, it creates a oneshot receiver and stores the sender in `waiters`. When `resolve()` is called, it sends the response through the channel.

✨ **Where Rust shines:** **No polling.** The waiter is suspended by the Tokio runtime until the oneshot fires. No CPU cycles wasted on `while not resolved: sleep(0.1)`. In Python, `asyncio.Event.wait()` is efficient too, but the oneshot pattern is more explicit about "exactly one response."

### The Overlay

In the TUI, approval requests appear as a modal overlay:

```rust
// File: octopus-cli/src/ui/shell/mod.rs
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

File: `octopus-cli/src/auth/manager.rs` (~270 lines)

The `OAuthManager` handles token storage, refresh, and credential resolution:

```rust
// File: octopus-cli/src/auth/manager.rs
pub struct OAuthManager {
    access_tokens: Arc<Mutex<HashMap<String, String>>>,
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
    rejected_refresh_tokens: Arc<Mutex<HashMap<String, (String, Instant)>>>,
}
```

### Layered Credential Model

Rust follows Python's pattern of keeping **config and runtime credentials separate**:

| Layer | Field | Mutable? | Source |
|-------|-------|----------|--------|
| **Config** | `provider_config.api_key` | ❌ No | Config file / env vars |
| **Config** | `provider_config.oauth` | ❌ No | Config file (OAuth reference) |
| **Runtime** | `OAuthManager` cache | ✅ Yes | Disk → memory cache |
| **Runtime** | `LLM.oauth` | ✅ Yes | Bound at `KimiSoul` startup |

The `LLM` stores an `OAuthManager` clone so that every `build_llm_provider()` call can resolve the **live** credential:

```rust
// File: octopus-cli/src/llm.rs
pub struct LLM {
    pub model_name: String,
    pub provider_config: Option<LLMProvider>,
    pub oauth: Option<OAuthManager>,  // ← runtime credential resolver
}
```

### Credential Resolution

`resolve_api_key` decides which credential to use **at provider-build time**:

```rust
// File: octopus-cli/src/auth/manager.rs
pub enum ApiCredential {
    OAuthToken(String),
    ApiKey(String),
}

pub fn resolve_api_key(
    &self,
    api_key: Option<String>,
    oauth_ref: Option<&OAuthRef>,
) -> Option<ApiCredential> {
    if let Some(ref_ref) = oauth_ref {
        let cache = self.access_tokens.lock().unwrap();
        if let Some(token) = cache.get(&ref_ref.key) {
            return Some(ApiCredential::OAuthToken(token.clone()));
        }
    }
    api_key.map(ApiCredential::ApiKey)
}
```

And `build_llm_provider` calls it before every LLM request:

```rust
// File: octopus-cli/src/llm.rs
fn resolve_api_key(&self) -> Option<String> {
    let provider_config = self.provider_config.as_ref()?;
    self.oauth
        .as_ref()
        .and_then(|o| o.resolve_api_key(
            provider_config.api_key.clone(),
            provider_config.oauth.as_ref(),
        ))
        .map(|c| c.as_str().to_string())
}
```

This means:
- **OAuth token takes priority** when available in the cache
- **Static API key is the fallback** when OAuth is not configured or the cache is empty
- **Config is never mutated** — `provider_config.api_key` stays as the static fallback forever

### Token Storage: Atomic & Permission-Safe

```rust
// File: octopus-cli/src/auth/oauth.rs
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
// File: octopus-cli/src/auth/manager.rs
pub async fn ensure_fresh(&self, llm: &LLM, force: bool) -> Result<bool> {
    let oauth_ref = match self.kimi_code_ref(llm) {
        Some(r) => r,
        None => return Ok(false),  // No OAuth configured
    };

    let token = match oauth::load_tokens(&oauth_ref.key) {
        Some(t) => t,
        None => return Ok(false),  // No persisted token
    };

    // Check tombstone (rejected refresh token)
    if self.should_suppress_persisted_token(&oauth_ref.key, &token) {
        if !self.can_retry_rejected_refresh_token(&oauth_ref.key, &token.refresh_token) {
            return Ok(false);  // Don't retry a known-bad token
        }
    }

    let _guard = self.refresh_lock.lock().await;  // Only one refresh at a time!
    self.refresh_tokens(&oauth_ref, token, force).await
}
```

`ensure_fresh` returns `Result<bool>`:
- `Ok(true)` — token was refreshed or is still valid
- `Ok(false)` — no OAuth configured, no token on disk, or refresh not needed yet
- `Err(...)` — refresh failed (e.g., refresh token rejected)

**Crucially, `ensure_fresh` does NOT return the token string.** It only updates the in-memory cache inside `OAuthManager`. The next call to `build_llm_provider()` will pick up the new token automatically via `resolve_api_key()`.

This is **resilient token management**:

1. **Proactive refresh** — refresh before expiry, not after
2. **Tombstones** — if a refresh token is rejected, don't retry it for 5 minutes
3. **Mutex lock** — only one refresh happens at a time, even with concurrent requests
4. **Threshold heuristic** — `max(300s, 0.5 * expires_in)` balances safety and API load
5. **Config immutability** — the static `api_key` in config is never overwritten

🐍 **Python's way:** `ensure_fresh` mutates the live HTTP client's `api_key` in-place (`runtime.llm.chat_provider.client.api_key = new_token`).

🦀 **Rust's way:** `ensure_fresh` mutates the `OAuthManager` cache only. The provider is rebuilt on every LLM call, so the fresh token is picked up naturally at `build_llm_provider()` time. No live client mutation needed.

✨ **Where Rust shines:** **No live object mutation.** Because `build_llm_provider()` creates a new LLM provider each time, credential resolution is a pure function of `LLM` state. In Python, mutating the HTTP client in-place creates a hidden side effect that can surprise callers holding references to the client.

---

## 🎭 The Yolo/Afk Modes

The Security Desk has three modes of operation:

| Mode       | Trigger | Behavior                             |
| ---------- | ------- | ------------------------------------ |
| **Normal** | Default | Every tool asks for approval         |
| **YOLO**   | `/yolo` | Auto-approve everything this session |
| **AFK**    | `/afk`  | Auto-approve; user is away           |

```rust
// File: octopus-cli/src/soul/approval.rs
pub struct ApprovalState {
    pub mode: ApprovalMode,
    pub auto_approve_actions: Vec<String>,  // Per-tool auto-approve list
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApprovalMode {
    Ask,        // Normal interactive mode
    Yolo,       // Auto-approve everything
    Afk,        // Auto-approve; user is away
    YoloAndAfk, // Both flags active simultaneously
}
```

These states are **persisted to the session** and synced at the end of every turn:

```rust
// File: octopus-cli/src/soul/kimisoul.rs
fn sync_approval_state(&mut self) {
    self.session.state.approval.mode = self.approval.state().mode;
    self.session.state.approval.auto_approve_actions = self.approval.auto_approve_actions();
    let _ = self.session.save_state();
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
