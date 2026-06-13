# qqbot

A bundled QQ bot solution for Linux servers. `qqbot` starts and manages two processes:

1. **NapCatQQ** — the QQ protocol layer (NTQQ + OneBot 11).
2. **qqbot-core** — a general bot runtime that loads Wasm plugins.

The business logic lives in **Wasm plugins**, so the same `qqbot` distribution can switch behaviors by swapping `.wasm` plugin files.

## Architecture

```text
┌─────────────────────────────────────────────┐
│                 qqbot                       │
│  (supervisor: configures/starts/monitors    │
│   NapCatQQ + qqbot-core)                    │
└──────┬──────────────────────┬───────────────┘
       │                      │
       ▼                      ▼
┌──────────────┐      ┌─────────────────┐
│   NapCatQQ   │      │   qqbot-core    │
│   process    │      │   (bot runtime) │
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

### 2. Prepare NapCatQQ

Download the Linux x64 **Shell** version of NapCatQQ from the [NapCatQQ releases](https://github.com/NapNeko/NapCatQQ/releases) page and extract it to `./napcat`.

The release tarball of `qqbot` may also ship with NapCatQQ pre-bundled.

### 3. Set up

```bash
./target/release/qqbot setup \
  --account 123456789 \
  --kimi-key sk-xxxxxx \
  --group 987654321 \
  --data-dir ./qqbot-data
```

This creates:

- `./qqbot-data/qqbot.toml` — supervisor config.
- `./qqbot-data/config.toml` — `qqbot-core` config.
- `./qqbot-data/napcat/app/napcat/config/onebot11_<account>.json` — NapCatQQ OneBot config.
- `./qqbot-data/plugins/` — plugin directory.

Copy the plugin into the plugin directory:

```bash
cp target/wasm32-unknown-unknown/release/summary.wasm ./qqbot-data/plugins/
```

### 4. Start

```bash
./target/release/qqbot start --data-dir ./qqbot-data
```

`qqbot` starts NapCatQQ, waits for its OneBot WebSocket port, then starts `qqbot-core`.

### 5. Log in

Open the NapCatQQ WebUI (usually `http://localhost:6099/webui`) and scan the QR code to log in the bot QQ account.

### 6. Test

In the allowed QQ group, send:

```
/summary
```

The bot buffers messages and asks the configured LLM to summarize the recent conversation.

## Commands

```bash
qqbot setup --account <QQ> --kimi-key <KEY> [--group <ID>]... [--data-dir <DIR>]
qqbot start [--data-dir <DIR>]
qqbot stop [--data-dir <DIR>]
qqbot status [--data-dir <DIR>]
```

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
