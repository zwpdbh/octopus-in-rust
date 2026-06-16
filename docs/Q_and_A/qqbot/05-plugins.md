# 4. WASM Plugin System

Octopus uses WebAssembly plugins as portable, sandboxed tools. The same plugin
binary can be loaded by the `brain` agent core, by `octopus-cli`, and by
`qqbot-core`. All three hosts share one plugin ABI, one runtime (Extism), and
one manifest format.

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
| `plugins/summary` | Formats a conversation log for summarization | `register_tools` + `execute` |
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
#[plugin_fn]
pub fn register_tools(_input: String) -> FnResult<String> {
    let tools = vec![ToolDef {
        name: "summary_format_conversation".to_string(),
        description: "Format a raw conversation log for summarization.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "messages": { "type": "string" },
                "style": { "type": "string", "default": "bullet" }
            },
            "required": ["messages"]
        }),
    }];
    Ok(serde_json::to_string(&tools)?)
}
```

A single `.wasm` file can register multiple tools. The host loads each one as a
separate callable tool. Names are usually namespaced, e.g.
`summary_format_conversation`.

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
  "tool": "summary_format_conversation",
  "arguments": {
    "messages": "123: hello\n456: world",
    "style": "bullet"
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

`qqbot` is a supervisor around two binaries:

- `apps/qqbot-core` — the bot runtime that connects to OneBot and runs Brain
  turns.
- `apps/qqbot` — the supervisor CLI that starts/stops the runtime and manages
  plugins.

### Where plugins live

- **Source builds:** `target/wasm32-unknown-unknown/release/<name>.wasm`
- **Enabled plugins:** `data/qqbot-data/plugins/<name>.wasm`

`qqbot-core` reads its plugin directory from `config.toml`:

```toml
[bot]
plugin_dir = "./data/qqbot-data/plugins"
```

`GroupBrainManager` passes that directory to `brain::ExtismPluginSource`:

```rust
let tool_sources: Vec<Arc<dyn brain::ToolSource>> = vec![
    Arc::new(ExtismPluginSource::new(&self.plugin_dir)),
];
```

So `qqbot-core` does **not** have its own plugin host — it reuses the one in
`brain`.

### Enabling, disabling, and reloading

`apps/qqbot/src/plugins.rs` implements the CLI operations:

```bash
cargo run --bin qqbot -- plugin list
cargo run --bin qqbot -- plugin enable summary
cargo run --bin qqbot -- plugin disable summary
cargo run --bin qqbot -- plugin reload
```

- `enable` copies the `.wasm` from `target/wasm32-unknown-unknown/release/` into
  the enabled directory.
- `disable` removes the `.wasm` from the enabled directory.
- `reload` reads the `qqbot-core` PID from `data/run/qqbot-core.pid` and sends
  `SIGHUP`.

When `qqbot-core` receives `SIGHUP`, it clears all cached `Brain` instances in
`GroupBrainManager`. The next message to each group creates a fresh `Brain` with
the current set of plugins.

### The default `summary` plugin

The enabled `summary.wasm` registers `summary_format_conversation`. When a user
runs `/summary`, `qqbot-core` builds a prompt like:

```text
Please summarize the recent conversation in this group.
```

and runs a Brain turn. The Brain may call `qqbot_recent_messages` (a host tool)
to fetch raw messages, then call `summary_format_conversation` to format them,
and finally produce a summary for the group.

---

## 4.7 Writing a plugin

The easiest way is to copy `plugins/summary` or `plugins/example-http`.

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

## 4.8 Build and install checklist

```bash
# Build a plugin
cargo build --release -p my-plugin --target wasm32-unknown-unknown

# For octopus-cli (manual install until CLI commands are implemented)
mkdir -p ~/.kimi/plugins
cp target/wasm32-unknown-unknown/release/my-plugin.wasm ~/.kimi/plugins/
cp plugins/my-plugin/manifest.json ~/.kimi/plugins/my-plugin.json

# For qqbot
cargo run --bin qqbot -- plugin enable my-plugin
```

Reloading:

- `octopus-cli`: plugins are loaded at startup; restart the CLI.
- `qqbot`: `cargo run --bin qqbot -- plugin reload` (or enable/disable, which
  reloads automatically).

---

## 4.9 Security and sandboxing

- Plugins run inside Extism with a per-call memory limit and timeout.
- Network and filesystem access are **deny-by-default**.
- Manifest permissions are the only way to grant access; never add a wildcard
  `allowed_hosts` unless you trust the plugin.
- Each tool call instantiates a fresh `Plugin` from the compiled module, so a
  crashing call cannot corrupt later calls.

---

## 4.10 Legacy ABI note

Older drafts of this project used a C-like ABI with exports such as `init`,
`on_message`, `on_command`, `malloc`, and `free`. That ABI is **no longer used**
by `brain`, `octopus-cli`, or `qqbot-core`. The `summary` plugin still contains
those exports only as a compatibility shim; the active path is the Extism-based
`register_tools` / `execute` ABI described above.
