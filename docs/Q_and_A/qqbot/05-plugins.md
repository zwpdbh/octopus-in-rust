# 4. WASM Plugin System

Octopus uses WebAssembly plugins as portable, sandboxed tools. The same plugin
binary can be loaded by the `brain` agent core, by `octopus-cli`, and by
`qqbot-core`. All three hosts share one plugin ABI, one runtime (Extism), and
one manifest format.

In `qqbot`, plugins are managed like OS programs: you **register** a built
`.wasm` to install it, **update** it by registering a newer build, **unregister**
it to remove it, and **list** the tools actually loaded in the running core.

This document explains the shared ABI and then shows how each host discovers,
loads, and runs plugins.

---

## 4.1 What a plugin looks like

A plugin is a `.wasm` file compiled for `wasm32-unknown-unknown` with
`crate-type = ["cdylib"]`. It is built with the
[Extism PDK](https://extism.org/docs/concepts/pdk/) and runs inside the
[Extism](https://extism.org/) runtime with WASI enabled.

Two example plugins live in the workspace:

| Plugin | Purpose | Key ABI |
|--------|---------|---------|
| `plugins/faf-units` | Query and compare FAF units | `register_tools` + `execute` |
| `plugins/example-http` | Makes outbound HTTP requests | `tool_metadata` + `execute` |

---

## 4.2 The shared Brain plugin ABI

A plugin exposes one or more exports. The host tries them in this order:

| Export | Required? | Purpose |
|--------|-----------|---------|
| `register_tools` | No | Returns a JSON array of tool definitions. Preferred modern ABI. |
| `tool_metadata` | No | Returns a single tool definition. Legacy-but-stable ABI. |
| `execute` | **Yes** | Receives a JSON payload and returns a JSON result. |

If neither metadata export is present, the host falls back to the filename as
the tool name and an empty parameter schema.

### `register_tools` (preferred)

```rust
// plugins/faf-units/src/lib.rs ~line 52 — register_tools
#[plugin_fn]
pub fn register_tools(_input: String) -> FnResult<String> {
    let tools = vec![
        ToolDef {
            name: "faf_units_search".to_string(),
            description: "Search FAF units by id, name, description or category.".to_string(),
            prompt_fragment: Some(
                "When the user asks about FAF units, use faf_units_search.".to_string(),
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "default": 10 }
                },
                "required": ["query"]
            }),
        },
        ToolDef {
            name: "faf_units_compare".to_string(),
            description: "Compare two FAF units side-by-side.".to_string(),
            prompt_fragment: None,
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id_a": { "type": "string" },
                    "id_b": { "type": "string" }
                },
                "required": ["id_a", "id_b"]
            }),
        },
    ];
    Ok(serde_json::to_string(&tools)?)
}
```

A single `.wasm` file can register multiple tools. The host loads each one as a
separate callable tool. Names are usually namespaced, e.g. `faf_units_search`.

### `tool_metadata` (single-tool fallback)

```rust
#[plugin_fn]
pub fn tool_metadata() -> FnResult<String> {
    Ok(r#"{
        "name": "HttpRequest",
        "description": "Make HTTP requests...",
        "schema": { "type": "object", "properties": { ... } }
    }"#.to_string())
}
```

### `execute`

When the LLM calls a plugin tool, the host serializes the call into JSON and
invokes `execute`:

```json
{
  "tool": "faf_units_search",
  "arguments": {
    "query": "UEF tech1 tank",
    "limit": 5
  }
}
```

The plugin returns an arbitrary JSON string. Errors are returned as JSON with an
`error` field or by setting `is_error` through the Extism result code (the host
also accepts a plain string result).

---

## 4.3 The plugin manifest

Alongside each `.wasm` file you can place a `<name>.json` manifest. The manifest
is authoritative for metadata and security. If it is missing, the host tries the
`register_tools` / `tool_metadata` exports and uses deny-by-default security.

Example: `plugins/example-http/manifest.json`

```json
{
  "name": "HttpRequest",
  "description": "Make HTTP requests to external APIs and websites...",
  "prompt_fragment": "When the user asks for data from the web, prefer HttpRequest over guessing.",
  "schema": { "type": "object", "properties": { ... } },
  "allowed_hosts": ["httpbin.org", "*.httpbin.org"],
  "timeout_ms": 30000,
  "max_memory_pages": 128
}
```

Security model: **deny-by-default**.

| Field | Meaning |
|-------|---------|
| `allowed_hosts` | Hosts the plugin may reach over HTTP. `None` or `[]` means no network. |
| `prompt_fragment` | Optional instruction appended to the system prompt by `ToolAwareSystemPromptPolicy`. |
| `allowed_paths` | Host-to-guest path mappings for WASI filesystem access. |
| `timeout_ms` | Maximum execution time. |
| `max_memory_pages` | Maximum WASM memory, in 64 KiB pages. |

If no manifest is provided, the host calls `disallow_all_hosts()` so the plugin
cannot make network requests.

---

## 4.4 How plugins work in `brain`

`crates/brain` is the reusable agent core. Plugin support lives in
`crates/brain/src/tools/plugin/`.

### Key types

- `WasmPluginTool` — a `kosong::tooling::CallableTool` backed by a compiled WASM
  module.
- `discover_plugins(dir)` — scans a directory for `.wasm` files and returns a
  `Vec<Box<dyn CallableTool>>`.
- `ExtismPluginSource` — implements `ToolSource` so plugins can be injected into
  a `Brain` through configuration.
- `PluginManifest` — the JSON manifest deserialized into Rust.

### Loading flow

1. `Brain::new(config)` iterates over `config.tool_sources`.
2. For each `ExtismPluginSource`, it calls `load_tools()`.
3. `load_wasm_plugin(path)`:
   - reads the `.wasm` bytes;
   - reads `<name>.json` if present;
   - builds an Extism `Manifest` with the security restrictions;
   - compiles the plugin with `PluginBuilder::new(...).with_wasi(true).compile()`;
   - resolves metadata from the manifest, then `register_tools`, then
     `tool_metadata`, then the filename;
   - returns a `WasmPluginTool`.
4. The tool is registered in the `ToolRegistry`.
5. During a turn, `kosong::step()` sees the tool definition, the LLM may call it,
   and `ToolRegistry::handle()` routes the call to `WasmPluginTool::call_raw()`.
6. `call_raw()` spawns a blocking task, instantiates a fresh `Plugin` from the
   pre-compiled module, and calls `execute` with the JSON input.

### Using plugins in a Brain programmatically

```rust
use brain::{Brain, BrainConfig, BrainBuilder, ExtismPluginSource};
use std::sync::Arc;

let tool_sources: Vec<Arc<dyn brain::ToolSource>> = vec![
    Arc::new(ExtismPluginSource::new("./data/qqbot-data/plugins")),
];

let config = BrainConfig {
    system_prompt: "You are a helpful assistant.".to_string(),
    tool_sources,
    ..Default::default()
};

let brain = BrainBuilder::default().from_config(config).build().await?;
```

This is exactly what `qqbot-core` does.

---

## 4.5 How plugins work in `octopus-cli`

`apps/octopus-cli` mirrors the Python `kimi-cli` and loads plugins as ordinary
tools that the LLM can call.

### Discovery

`apps/octopus-cli/src/plugin/discovery.rs` provides `discover_plugins()` and
`default_plugins_dir()`, which resolves to `~/.kimi/plugins`.

### Loading flow

Plugins are loaded in two places:

1. `Agent::new_basic()` — the default agent used when no agent file is given.
2. `load_agent()` — when an agent YAML spec is provided.

Both call:

```rust
if let Some(ref plugins_dir) = crate::plugin::default_plugins_dir() {
    if plugins_dir.is_dir() {
        for plugin_tool in crate::plugin::discover_plugins(plugins_dir) {
            // skip conflicts with built-in tools
            toolset.register(plugin_tool);
        }
    }
}
```

The plugin tools are added to the agent's `KimiToolset`, and the LLM receives
their names, descriptions, and JSON schemas just like built-in tools.

### CLI management (current status)

`octopus-cli` has a `kimi plugin` subcommand scaffold, but the operations are
still stubs:

```bash
# The binary is octopus-cli; the CLI internally calls itself "kimi".
octopus-cli plugin install <target>   # not yet implemented
octopus-cli plugin list               # not yet implemented
octopus-cli plugin remove <name>      # not yet implemented
octopus-cli plugin info <name>        # not yet implemented
```

Until those commands land, install plugins manually:

```bash
# 1. Build the plugin
cargo build --release -p example-http --target wasm32-unknown-unknown

# 2. Copy the .wasm (and optional .json manifest) into the CLI plugin dir
mkdir -p ~/.kimi/plugins
cp target/wasm32-unknown-unknown/release/example-http.wasm ~/.kimi/plugins/
cp plugins/example-http/manifest.json ~/.kimi/plugins/example-http.json

# 3. Run the CLI in print mode
octopus-cli --print -p "fetch https://httpbin.org/get"
```

---

## 4.6 How plugins work in `qqbot`

`qqbot` is an OS-like service manager around two binaries:

- `apps/qqbot-core` — the bot runtime that connects to OneBot and runs Brain
  turns.
- `apps/qqbot` — the supervisor CLI that starts/stops the runtime and
  **installs, upgrades, and removes plugin tools**.

### Installation model: Brain as OS, plugins as programs

After `qqbot init`, the data directory is remembered in `<project-root>/.qqbot`,
so most commands do not need `-d`.

| Operation | OS analogy | Command |
|---|---|---|
| Install a program | Copy binary into the system lib directory | `qqbot tools register <path>` |
| Upgrade a program | Overwrite the installed binary | `qqbot tools update <path>` |
| Uninstall a program | Remove the installed binary | `qqbot tools unregister <name>` |
| List running programs | Query the running OS | `qqbot tools list` |
| Check system status | `systemctl status` | `qqbot status` |

The `.wasm` file stem is the install key. To upgrade `faf_units_plugin`, keep the file
name `faf_units_plugin.wasm`.

### What happens during `tools register`

```rust
// apps/qqbot/src/plugins.rs ~line 62 — plugins::register (abbreviated)
pub async fn register(data_dir: &Path, wasm_path: &Path) -> Result<String> {
    // 1. Validate the .wasm with the same brain loader qqbot-core uses.
    let info = brain::tools::plugin::inspect_wasm_plugin(wasm_path).map_err(...)?;

    // 2. Copy into the plugin directory, overwriting an existing install.
    let dst = plugin_dir(data_dir).join(format!("{name}.wasm"));
    tokio::fs::copy(wasm_path, &dst).await?;

    // 3. Signal qqbot-core to reload if it is running.
    let pid_file = run_dir(data_dir).join("qqbot-core.pid");
    if pid_file.exists() {
        reload(data_dir).await?;
    }
    Ok(name)
}
```

Validation happens before the file is copied, so a malformed plugin never
reaches the runtime.

### Prompt fragments from tools

Tools can contribute instruction fragments to the system prompt. Host tools
implement `CallableTool2::prompt_fragment()`; WASM plugins declare
`prompt_fragment` in their manifest or `register_tools` export. When
`ToolAwareSystemPromptPolicy` builds the system prompt, it collects all fragments
from the currently registered tools and appends them under a
`### Tool usage instructions` heading.

This replaces hard-coded tool names in `qqbot-core`: the Brain now asks every
loaded tool, *"How should I use you?"*, and composes the answer into the prompt.

### Runtime control socket

`qqbot-core` exposes a Unix domain control socket at
`<data-dir>/../run/qqbot-core.sock`. The CLI uses it to ask the runtime what
tools are actually loaded, instead of guessing from files on disk.

```rust
// apps/qqbot-core/src/control.rs ~line 28 — control::serve (abbreviated)
pub async fn serve(config_path: PathBuf, manager: Arc<GroupBrainManager>) -> Result<()> {
    let socket_path = socket_path(&config_path).context(...)?;
    let listener = UnixListener::bind(&socket_path)?;
    info!(path = %socket_path.display(), "control socket listening");
    loop {
        let (mut stream, _) = listener.accept().await?;
        // read ControlRequest, dispatch, write ControlResponse
    }
}
```

`GroupBrainManager` reports the tools currently loaded in active Brains:

```rust
// apps/qqbot-core/src/group_brain.rs ~line 136 — GroupBrainManager::loaded_tool_names (abbreviated)
pub async fn loaded_tool_names(&self) -> Vec<String> {
    let brains = self.brains.lock().await;
    let mut names = std::collections::BTreeSet::new();
    for brain in brains.values() {
        for tool in brain.registry().tools() {
            names.insert(tool.name);
        }
    }
    names.into_iter().collect()
}
```

Brains are created eagerly at startup and re-created after each reload, so
`tools list` should reflect the installed plugins once `qqbot-core` has finished
initializing.

### Commands

```bash
# Install (or upgrade) from a built wasm binary.
cargo run --bin qqbot -- tools register target/wasm32-unknown-unknown/release/faf_units_plugin.wasm

# Same behavior as register, but semantically clearer for upgrades.
cargo run --bin qqbot -- tools update target/wasm32-unknown-unknown/release/faf_units_plugin.wasm

# Remove a plugin by its file-stem name.
cargo run --bin qqbot -- tools unregister faf_units_plugin

# Query the runtime; falls back to the plugin directory if core is not running.
cargo run --bin qqbot -- tools list

# Verify in the status output.
cargo run --bin qqbot -- status
```

### Reload semantics

`register`, `update`, and `unregister` send `SIGHUP` to `qqbot-core` when it is
running. `qqbot-core` clears all cached Brains and immediately re-creates them
with the current plugin directory contents.

The older `plugin enable / disable / reload` commands are still available for the
crate-name-based workflow, but `tools register/update/unregister` are preferred
because they validate the wasm explicitly and can target any file path.

### The default `faf-units` plugin

The installed `faf_units_plugin.wasm` registers tools such as `faf_units_search`,
`faf_units_get`, `faf_units_compare`, and `faf_units_naive_dps`. When a user asks
about FAF units, the Brain can search the embedded unit index and compare stats
without needing network access.

---

## 4.7 Per-group skills

Because each QQ group gets its own `Brain`, skills can be configured per group.
A skill here is a combination of:

- a group-specific **system prompt**;
- a whitelist of **enabled plugins**.

### Profile storage

Each group has a TOML file at:

```text
data/qqbot-data/groups/<group_id>.toml
```

Example `data/qqbot-data/groups/925712027.toml`:

```toml
system_prompt = "You are a concise assistant for this gaming group."
enabled_plugins = ["faf_units_plugin"]
```

`enabled_plugins` is a whitelist: only the listed plugin file stems are loaded
for the group. If the list is omitted or empty, no plugins are loaded.

### CLI

```bash
# Show a group's effective profile
cargo run --bin qqbot -- group 925712027 show

# Set a group-specific system prompt
cargo run --bin qqbot -- group 925712027 set-prompt "You are a concise assistant for this gaming group."

# Add a plugin to a group's whitelist
cargo run --bin qqbot -- group 925712027 enable-plugin summary
```

Changes write the TOML file and send `SIGHUP` to `qqbot-core`, so the group's
next Brain is created with the new skill set.

### Loading flow in `qqbot-core`

```rust
// apps/qqbot-core/src/group_brain.rs ~line 258 — GroupBrainManager::create_brain (abbreviated)
async fn create_brain(&self, group_id: i64) -> Result<Brain> {
    let profile = match qqbot_config::GroupProfile::load(&self.data_dir, group_id) {
        Ok(Some(p)) => p,
        _ => qqbot_config::GroupProfile::default(),
    };

    // Only load plugins listed in the group's whitelist.
    let installed = Self::installed_plugin_names(&self.plugin_dir);
    let allowed = profile.filter_plugins(installed.iter().map(|s| s.as_str()));
    let tool_source: Arc<dyn brain::ToolSource> =
        Arc::new(ExtismPluginSource::with_filter(&self.plugin_dir, allowed));

    let config = BrainConfig {
        system_prompt: profile
            .system_prompt
            .clone()
            .unwrap_or_else(|| self.config.llm.system_prompt.clone()),
        // ...
    };
    // ...
}
```

The shared `qqbot-config` crate (`crates/qqbot-config`) holds `GroupProfile` so
both the CLI and `qqbot-core` agree on the file format.

---

## 4.8 Writing a plugin

The easiest way is to copy `plugins/faf-units` or `plugins/example-http`.

### `Cargo.toml`

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
extism-pdk = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### Minimal `execute`-only plugin

```rust
use extism_pdk::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Args {
    name: String,
}

#[plugin_fn]
pub fn execute(input: String) -> FnResult<String> {
    let args: Args = serde_json::from_str(&input)?;
    Ok(format!("Hello, {}!", args.name))
}
```

Without a manifest or metadata export, the host will name this tool `my-plugin`
(from the filename) and present it with no parameters. Add a `my-plugin.json`
manifest or a `tool_metadata` export to describe the schema.

### Multi-tool plugin

Use `register_tools` when one `.wasm` provides several related tools:

```rust
#[plugin_fn]
pub fn register_tools(_input: String) -> FnResult<String> {
    let defs = vec![
        ToolDef { name: "my::foo".into(), description: "...".into(), parameters: ... },
        ToolDef { name: "my::bar".into(), description: "...".into(), parameters: ... },
    ];
    Ok(serde_json::to_string(&defs)?)
}

#[plugin_fn]
pub fn execute(input: String) -> FnResult<String> {
    let call: ExecuteInput = serde_json::from_str(&input)?;
    match call.tool.as_str() {
        "my::foo" => do_foo(call.arguments),
        "my::bar" => do_bar(call.arguments),
        _ => Ok(format!("Unknown tool: {}", call.tool)),
    }
}
```

### HTTP plugins

If your plugin calls external APIs, declare `allowed_hosts` in the manifest or
Extism will block the request.

---

## 4.9 Build and install checklist

```bash
# Build a plugin
cargo build --release -p my-plugin --target wasm32-unknown-unknown

# For octopus-cli (manual install until CLI commands are implemented)
mkdir -p ~/.kimi/plugins
cp target/wasm32-unknown-unknown/release/my-plugin.wasm ~/.kimi/plugins/
cp plugins/my-plugin/manifest.json ~/.kimi/plugins/my-plugin.json

# For qqbot — install, upgrade, and remove like OS packages
# (after qqbot init these work without -d from anywhere in the project)
cargo run --bin qqbot -- tools register target/wasm32-unknown-unknown/release/my-plugin.wasm
cargo run --bin qqbot -- tools update   target/wasm32-unknown-unknown/release/my-plugin.wasm
cargo run --bin qqbot -- tools unregister my-plugin
cargo run --bin qqbot -- tools list
cargo run --bin qqbot -- status
```

Reloading:

- `octopus-cli`: plugins are loaded at startup; restart the CLI.
- `qqbot`: `tools register` / `tools update` / `tools unregister` reload
  automatically via `SIGHUP`. You can also run
  `cargo run --bin qqbot -- plugin reload` for a manual reload.

---

## 4.10 Security and sandboxing

- Plugins run inside Extism with a per-call memory limit and timeout.
- Network and filesystem access are **deny-by-default**.
- Manifest permissions are the only way to grant access; never add a wildcard
  `allowed_hosts` unless you trust the plugin.
- Each tool call instantiates a fresh `Plugin` from the compiled module, so a
  crashing call cannot corrupt later calls.

---

## 4.11 Legacy ABI note

Older drafts of this project used a C-like ABI with exports such as `init`,
`on_message`, `on_command`, `malloc`, and `free`. That ABI is **no longer used**
by `brain`, `octopus-cli`, or `qqbot-core`. The active path is the Extism-based
`register_tools` / `execute` ABI described above.
