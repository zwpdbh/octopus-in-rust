# Tour 1: The Lobby — Where Every Visit Begins

> *"Every grand building has a lobby. This one has a CLI parser."*

Welcome to the **Lobby** of Octopus-CLI! This is where visitors (users) first enter the building. The lobby handles three things:
1. **Reading your invitation** — parsing command-line arguments
2. **Checking your credentials** — OAuth login/logout
3. **Directing you to the right floor** — dispatching to TUI shell, print mode, or subcommands

---

## 🚪 The Main Entrance: `main.rs`

File: `octopus-cli/src/main.rs` (~430 lines)

In the Python original, the entrance was split across `__main__.py`, `cli/__init__.py`, and `cli/__main__.py`. In Rust, everything converges into a single `main.rs` — no package init ceremony, no `if __name__ == "__main__"` dance.

### The Door Opens

```rust
fn main() {
    let cli = <Cli as clap::Parser>::parse();
    // ...
}
```

🐍 **Python's way:** Click decorators, lazy command groups, and dynamic subcommand loading.

🦀 **Rust's way:** `clap`'s derive macro generates the entire CLI schema at compile time. No runtime reflection needed.

✨ **Where Rust shines:** The CLI schema is **validated at compile time**. If you rename a field in `Cli` but forget to update a reference, the compiler catches it. In Python, you'd only find out at runtime when a user types `--help`.

### The Grand Staircase: Runtime Dispatch

```rust
let result = match ui_mode {
    UiMode::Print => instance.run_print(...).await,
    UiMode::Acp   => instance.run_acp().await,
    UiMode::Wire  => instance.run_wire_stdio().await,
    UiMode::Shell => instance.run_shell(prompt.clone(), None).await,
};
```

This is the **central dispatcher**. Notice how it consumes `instance` — the `OctopusCLI` struct owns the `KimiSoul` and gives it away to whichever UI mode runs. This is Rust's ownership system in action: only one UI can run at a time, and the compiler enforces it.

---

## 🏛️ The Concierge Desk: `app.rs`

File: `octopus-cli/src/app.rs` (~250 lines)

If `main.rs` is the doorman, `app.rs` is the concierge who prepares your room key (the `Runtime`) and hands it to the bellhop (the UI).

### Building the Runtime

```rust
pub struct OctopusCLI {
    pub soul: Option<KimiSoul>,
    pub runtime: Runtime,
    pub env_overrides: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Runtime {
    pub config: Config,
    pub session: Session,
    pub llm: Option<LLM>,
    pub approval: ApprovalRuntime,
    pub ui_mode: UiMode,
    pub resumed: bool,
}
```

🐍 **Python's way:** The `App` class in `app.py` (~807 lines) dynamically assembles providers, handles config reloading, and manages UI mode switching with exception-based flow control (`Reload`, `SwitchToWeb`).

🦀 **Rust's way:** A plain struct with explicit fields. No magic methods, no dynamic assembly. The `Runtime` is built once and passed around by value or reference.

✨ **Where Rust shines:** **No hidden state mutations.** In Python, `app.config` could change mid-flight via a reload hook. In Rust, `config` is owned by `Runtime`, and you must explicitly opt into mutation. This makes the data flow **visible** to both the compiler and the reader.

### The Soul Factory

```rust
let llm = create_llm(&provider, &model)?;
let approval_state = session.state.approval_state();
let soul = KimiSoul::new(config, session, Some(llm), approval_state);
```

The `KimiSoul` is the heart of the building — we'll visit it in Tour 2. Here in the lobby, it's constructed with **all dependencies injected upfront**. No global variables, no `threading.local()`, no late initialization.

---

## 🔑 The Credential Check: OAuth Login/Logout

File: `octopus-cli/src/auth/` — `mod.rs`, `oauth.rs`, `platforms.rs`

The lobby has a security checkpoint for credentials. Let's look at the OAuth flow:

### Device Flow in Action

```rust
// auth/oauth.rs
pub async fn login_kimi_code() -> Result<OAuthToken> {
    let device_auth = request_device_authorization().await?;
    println!("Please visit: {}", device_auth.verification_uri);
    println!("Enter code: {}", device_auth.user_code);
    poll_device_token(&device_auth.device_code, device_auth.interval, ...).await
}
```

🐍 **Python's way:** `requests` for HTTP, `keyring` for secure storage, async with `aiohttp`.

🦀 **Rust's way:** `reqwest` for HTTP, atomic file writes (`0o600` permissions) for token storage, async with native `tokio`.

✨ **Where Rust shines:** **Token storage is inherently safe.** The Python version stores tokens in a file too, but Rust's `std::fs::OpenOptions` lets us set permissions atomically at creation time:

```rust
let file = OpenOptions::new()
    .mode(0o600)  // Only owner can read/write
    .create(true)
    .write(true)
    .open(&path)?;
```

In Python, you'd create the file, then call `os.chmod()` — a race window exists between creation and permission setting. Rust closes that window.

---

## 🛗 The Elevator: Choosing Your Floor

The lobby's elevator buttons correspond to CLI subcommands:

| Button | Command | Destination |
|--------|---------|-------------|
| 🏠 Default | `kimi` | TUI Shell (Tour 7) |
| 📄 `--print` | `kimi --print` | Print mode — stdin → LLM → stdout |
| 🔑 `login` | `kimi login` | OAuth device flow |
| 📤 `export` | `kimi export` | Session export |

Notice that **`--session` resumes existing sessions**. The lobby looks up the session directory, restores `context.jsonl`, and hands a fully-loaded `KimiSoul` to the UI.

---

## 🗂️ The Filing Cabinet: `Config`

File: `octopus-cli/src/config.rs` (~433 lines)

Before you can enter, the lobby loads your preferences from `~/.kimi/config.toml`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub default_model: String,
    pub providers: HashMap<String, LLMProvider>,
    pub hooks: Vec<HookConfig>,
    pub theme: String,
    pub loop_control: LoopControl,
    pub notifications: NotificationConfig,
    pub extra_skill_dirs: Vec<String>,
    pub merge_all_available_skills: bool,
}
```

🐍 **Python's way:** `pydantic` models with validators, `.env` file support, and live reloading.

🦀 **Rust's way:** `serde` + `toml` for deserialization. Validation is manual or via the `validator` crate. No live reloading (yet) — config is loaded once at startup.

✨ **Where Rust shines:** **Zero-cost config.** The `Config` struct is laid out exactly in memory as specified. No `__dict__` overhead, no runtime attribute lookup. A `Config` is ~200 bytes on the stack. In Python, the equivalent Pydantic model is a heap-allocated object with vtable indirection.

---

## 🎁 Souvenir Shop: What to Remember

1. **The lobby is thin.** `main.rs` + `app.rs` are ~680 lines combined. Python's equivalent was ~1,200 lines spread across 4 files.
2. **Ownership is visible.** The `OctopusCLI` struct owns `soul: Option<KimiSoul>`. When the UI runs, the soul is `take()`n. The compiler ensures no double-use.
3. **No runtime magic.** CLI parsing, config loading, and session setup are all **explicit function calls**. No decorators, no metaclasses, no import-side effects.

---

## 🚶 Next Stop

The lobby has handed you your room key. Now let's take the elevator to the **Control Room** — where `KimiSoul` makes all the decisions.

→ [Tour 2: The Control Room](./02-control-room.md)
