# 4. Plugin Management

Plugins are WebAssembly modules. `qqbot-core` discovers every `.wasm` file in the configured plugin directory at startup and reloads them on `SIGHUP`.

## 4.1 Where plugins live

- **Source builds:** `target/wasm32-unknown-unknown/release/<name>.wasm`
- **Enabled plugins:** `data/qqbot-data/plugins/<name>.wasm`

`qqbot` treats copying/removing files from the enabled directory as enabling/disabling a plugin. `qqbot-core` is then signaled to reload without restarting the daemon or the SnowLuma container.

## 4.2 List plugins

```bash
./target/release/qqbot plugin list
```

Output:

```text
summary              enabled
example-http         available
```

## 4.3 Enable a plugin

```bash
./target/release/qqbot plugin enable summary
```

This copies `target/wasm32-unknown-unknown/release/summary.wasm` into `data/qqbot-data/plugins/` and sends `SIGHUP` to `qqbot-core`.

## 4.4 Disable a plugin

```bash
./target/release/qqbot plugin disable summary
```

This removes the `.wasm` file and signals a reload.

## 4.5 Reload plugins

If you manually add or remove `.wasm` files, signal a reload:

```bash
./target/release/qqbot plugin reload
```

## 4.6 Writing a plugin

Plugins must export a C-like ABI:

- `init() -> i32`
- `on_message(event_ptr, event_len, out_ptr, out_cap) -> i32`
- `on_command(cmd_ptr, cmd_len, event_ptr, event_len, out_ptr, out_cap) -> i32`
- `malloc(size) -> *mut u8`
- `free(ptr, size)`

Input and output buffers are UTF-8 JSON. `on_command` receives the command name and the full OneBot message event. The plugin writes a JSON array of actions to the output buffer:

```json
[
  {"type": "send_group_msg", "group_id": 925712027, "text": "hello"},
  {"type": "log", "level": "info", "message": "handled /summary"},
  {"type": "llm_request", "group_id": 925712027, "prompt": "Summarize: ..."}
]
```

See `qqbot-plugins/summary` for the reference implementation.

## 4.7 Adding a new plugin to the workspace

1. Create a new crate under `qqbot-plugins/<name>/`.
2. Implement the required exports.
3. Build with `cargo build --release -p <name> --target wasm32-unknown-unknown`.
4. Enable it with `qqbot plugin enable <name>`.
