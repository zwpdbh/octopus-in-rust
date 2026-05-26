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

If `main.rs` is the doorman, `app.rs` is the concierge who prepares your room key (the `AppRuntime`) and hands it to the bellhop (the UI).

### Building the AppRuntime

```rust
pub struct OctopusCLI {
    pub soul: Option<KimiSoul>,
    pub runtime: AppRuntime,
    pub env_overrides: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AppRuntime {
    pub config: Config,
    pub session: Session,
    pub llm: Option<LLM>,
    pub approval: ApprovalRuntime,
    pub ui_mode: UiMode,
    pub resumed: bool,
}
```

🐍 **Python's way:** The `App` class in `app.py` (~807 lines) dynamically assembles providers, handles config reloading, and manages UI mode switching with exception-based flow control (`Reload`, `SwitchToWeb`).

🦀 **Rust's way:** A plain struct with explicit fields. No magic methods, no dynamic assembly. The `AppRuntime` is built once and passed around by value or reference.

✨ **Where Rust shines:** **No hidden state mutations.** In Python, `app.config` could change mid-flight via a reload hook. In Rust, `config` is owned by `AppRuntime`, and you must explicitly opt into mutation. This makes the data flow **visible** to both the compiler and the reader.

### The Soul Factory

`OctopusCLI::create()` is the lobby's **initialization pipeline**. It runs in 8 numbered phases, each building on the last:

```rust
pub async fn create(...) -> Result<Self> {
    // 1. Load configuration and apply CLI overrides.
    let mut config = match config_source { ... };
    // 1.1 ... 1.2 loop-control overrides ...

    // 2. Resolve model and provider.
    // 2.1 Look up explicit model and default model.
    // 2.2 Pre-compute existence booleans.
    // 2.3 Match on the tuple to pick model/provider with clear priority.
    // 2.4 Apply environment variable overrides.

    // 3. Resolve derived settings and instantiate LLM.
    // 4. Build approval state from session + CLI flags.
    // 5. Construct agent and soul (KimiSoul::new).
    // 6. Assemble the lightweight AppRuntime.
    // 7. Initialize telemetry.
    // 8. Return the fully initialized CLI handle.
}
```

The `KimiSoul` is the heart of the building — we'll visit it in Tour 2. Here in the lobby, it's constructed with **all dependencies injected upfront**. No global variables, no `threading.local()`, no late initialization.

Notice the **config resolution step**: `--config` (inline string) and `--config-file` (path) are modeled as a single `ConfigSource` enum. This follows the Rust principle of *making invalid states unrepresentable* — the compiler ensures you handle exactly one source, not zero and not two.

#### Model/Provider Resolution: Tuple Matching in Action

Phase 2 is where the lobby decides which LLM to talk to. Rather than a chain of `if` blocks that mutate `Option` locals, the code **pre-computes four booleans and matches on the tuple**:

```rust
let explicit = model_name.as_ref().and_then(|n| config.models.get(n));
let default  = config.models.get(&config.default_model);

let name_given    = model_name.is_some();
let name_exists   = explicit.is_some();
let default_given = !config.default_model.is_empty();
let default_exists = default.is_some();

let (mut model, mut provider) = match (name_given, name_exists, default_given, default_exists) {
    (true, true, _, _) => { /* use explicit model */ }
    (false, _, true, true) => { /* use default model */ }
    _ => { /* hard-coded fallback */ }
};
```

This is the **"precompute and match on tuples"** pattern from our style guide (`AGENTS.md`). It makes the scenario matrix explicit:

| `model_name` given? | Found in config? | `default_model` given? | Found in config? | Result |
|---|---|---|---|---|
| ✅ | ✅ | — | — | Use explicit model |
| ✅ | ❌ | — | — | Fallback |
| ❌ | — | ✅ | ✅ | Use default model |
| ❌ | — | ✅ | ❌ | Fallback |
| ❌ | — | ❌ | — | Fallback |

🐍 **Python's way:** Two overlapping `if` blocks that mutate local variables, followed by a fallback `if not model:`.

🦀 **Rust's way:** A single `match` on a 4-tuple. Every combination is visible in one place. No variable shadowing, no hidden mutation order.

✨ **Where Rust shines:** **The priority logic is exhaustive.** Add a new boolean dimension, and the compiler forces you to update every arm. In Python, a new condition might silently fall through to the wrong branch.

Notice also the **agent loading step**: the YAML spec drives which tools are registered, what system prompt is used, and which subagents are available. Plus:
- **WASM plugins** are discovered from `~/.kimi/plugins/`
- **MCP servers** are connected from CLI `--mcp-config` arguments
- **Subagent types** are registered in the `LaborMarket`

#### Agent Loading: What Changed from Python

This mirrors Python's `load_agent(agent_file, runtime)` architecture — loading an agent spec, building a toolset, registering subagents, and handling MCP servers — but **replaces the implementation mechanics**, not adds new capabilities:

| Mechanism | Python (`tmp/kimi-cli`) | Rust (`octopus-cli`) |
|---|---|---|
| **Tool loading** | Dynamic `importlib` import by module path (`kimi_cli.tools.shell:Shell`) with a dependency-injection dict | Static `match` on tool name string; each tool is constructed inline in `build_tool()` |
| **Plugins** | Directory-based: `~/.kimi/plugins/<name>/plugin.json` → native Python `PluginTool` instances | WASM-based: `~/.kimi/plugins/*.wasm` → Extism `WasmPluginTool` (sandboxed, language-agnostic) |
| **MCP deferral** | Optional via `start_mcp_loading: bool` flag (caller chooses immediate vs. deferred) | Mandatory (always deferred at load time; `start_deferred_mcp_tool_loading()` triggers it later) |

🐍 **Python's way:** `load_agent` receives a `start_mcp_loading` flag. If `True`, MCP servers spin up immediately in the background. If `False`, they're stashed in `_deferred_mcp_load` and started later. Plugins are discovered by scanning `~/.kimi/plugins/` for directories containing `plugin.json` manifests.

🦀 **Rust's way:** `load_agent` always defers MCP loading — no flag. The caller decides when to call `toolset.start_deferred_mcp_tool_loading()`. Plugins are discovered by scanning `~/.kimi/plugins/` for `.wasm` files (not directories), loaded via the Extism runtime. Tool construction is a hard-coded `match` rather than dynamic import.

✨ **Where Rust shines:** **WASM plugins are sandboxed.** A crashing or malicious plugin cannot corrupt the host process — Extism enforces memory isolation and capability restrictions. Python's native plugins run with full process privileges. The static `match` dispatcher also means tool loading has **zero runtime reflection cost**; the compiler knows every possible tool at build time.

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
4. **Match on tuples for multi-condition logic.** The model/provider resolution uses a 4-tuple `match` instead of `if-else` chains. This makes the scenario matrix explicit and guarantees every case is handled — a pattern enforced in `AGENTS.md`.

---

## 🚶 Next Stop

The lobby has handed you your room key. Now let's take the elevator to the **Control Room** — where `KimiSoul` makes all the decisions.

→ [Tour 2: The Control Room](./02-control-room.md)
