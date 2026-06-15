# qqbot

A bundled QQ bot solution for Linux servers. `qqbot` starts and manages two processes:

1. **SnowLuma** — the QQ protocol layer (NTQQ + OneBot 11), running inside Docker.
2. **qqbot-core** — a general bot runtime that loads Wasm plugins.

The business logic lives in **Wasm plugins**, so the same `qqbot` distribution can switch behaviors by swapping `.wasm` plugin files.

## Architecture

```text
┌─────────────────────────────────────────────┐
│                 qqbot                       │
│  (supervisor: configures/starts/monitors    │
│   SnowLuma + qqbot-core)                    │
└──────┬──────────────────────┬───────────────┘
       │                      │
       ▼                      ▼
┌──────────────┐      ┌─────────────────┐
│   SnowLuma   │      │   qqbot-core    │
│   (Docker)   │      │   (bot runtime) │
└──────┬───────┘      └────────┬────────┘
       │                       │
       │ OneBot 11             │ loads
       │ WebSocket             │ .wasm
       ▼                       ▼
┌─────────────────────────────────────────┐
│            Wasm plugins                 │
│  (summary, moderation, welcome, etc.)   │
└─────────────────────────────────────────┘
```

## Quick start

### 1. Build

```bash
cargo build --release -p qqbot -p qqbot-core
cargo build --release -p summary --target wasm32-unknown-unknown
```

### 2. Initialize

```bash
./target/release/qqbot init \
  --account 123456789 \
  --kimi-key sk-xxxxxx \
  --group 987654321
```

This creates the runtime data layout under `./data/`:

- `./data/qqbot-data/config.toml` — `qqbot-core` config
- `./data/qqbot-data/plugins/` — plugin directory
- `./data/snowluma-data/` — SnowLuma/QQ session state
- `./data/run/` — pid files
- `./data/logs/` — logs

### 3. Start

```bash
./target/release/qqbot start
```

`qqbot` starts SnowLuma in Docker, waits for its OneBot WebSocket port, then starts `qqbot-core`.

### 4. Log in

Open noVNC and scan the QR code:

```text
http://localhost:6081
password: vncpasswd
```

### 5. Add the bot to the group

`--group` configures a permission filter. You still need to add the bot QQ account as a member of the real QQ group from a normal QQ client.

### 6. Verify

```bash
./target/release/qqbot status
./target/release/qqbot health
```

### 7. Test

In the allowed QQ group, send:

```text
/summary
```

The bot buffers messages and asks the configured LLM to summarize the recent conversation.

## Commands

```bash
qqbot init --account <QQ> --kimi-key <KEY> [--group <ID>]... [--data-dir <DIR>]
qqbot start [--data-dir <DIR>]
qqbot stop [--data-dir <DIR>]
qqbot restart [--data-dir <DIR>]
qqbot status [--data-dir <DIR>]
qqbot health [--data-dir <DIR>]
qqbot doctor [--data-dir <DIR>]
qqbot logs [core|snowluma|supervisor] [-n N] [--data-dir <DIR>]
qqbot plugin list [--data-dir <DIR>]
qqbot plugin enable <name> [--data-dir <DIR>]
qqbot plugin disable <name> [--data-dir <DIR>]
qqbot plugin reload [--data-dir <DIR>]
qqbot reset [--data-dir <DIR>]
```

See `docs/Q_and_A/qqbot/` for the full tutorial.

## Writing a plugin

Plugins are WebAssembly modules compiled for `wasm32-unknown-unknown`. They must export:

- `init() -> i32`
- `on_message(event_ptr, event_len, out_ptr, out_cap) -> i32`
- `on_command(cmd_ptr, cmd_len, event_ptr, event_len, out_ptr, out_cap) -> i32`
- `malloc(size) -> *mut u8`
- `free(ptr, size)`

Input and output buffers are UTF-8 JSON. The output is an array of actions:

```json
[
  {"type": "send_group_msg", "group_id": 123, "text": "hello"},
  {"type": "log", "level": "info", "message": "..."},
  {"type": "llm_request", "group_id": 123, "prompt": "Summarize: ..."}
]
```

See `qqbot-plugins/summary` for a reference implementation.

## License

MIT
