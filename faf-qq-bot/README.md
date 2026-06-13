# faf-qq-bot

A cloud-deployable QQ group summary bot written in Rust. It connects to [NapCatQQ](https://github.com/NapNeko/NapCatQQ) via the OneBot 11 protocol, buffers group messages, and generates summaries on demand using the Kimi (Moonshot AI) API (OpenAI-compatible).

## Architecture

```text
┌─────────────────┐     OneBot 11      ┌──────────────────┐
│   faf-qq-bot    │ ◄────WebSocket────► │    NapCatQQ      │
│   (this crate)  │                     │  + NTQQ client   │
└─────────────────┘                     └──────────────────┘
```

- **NapCatQQ + NTQQ** handle login, receiving messages, and sending messages.
- **faf-qq-bot** handles the bot logic: buffering, command parsing, LLM calls, and replies.

## Why this design?

Pure QQ protocol libraries in Rust (such as `ricq`) are no longer actively maintained and are fragile against Tencent's protocol changes. NapCatQQ hooks the official NTQQ client, so it is currently the most reliable way to run a QQ bot. `faf-qq-bot` keeps the business logic in Rust while delegating the protocol to NapCatQQ.

## Quick start

### 1. Install NapCatQQ

Follow the [NapCatQQ documentation](https://napneko.github.io/) to install NapCatQQ and the official NTQQ Linux client. Make sure the OneBot 11 WebSocket adapter is enabled.

### 2. Configure the bot

```bash
cp config.example.toml config.toml
# Edit config.toml with your OneBot URL, group whitelist, and LLM credentials.
```

### 3. Run

If NapCatQQ is already running:

```bash
cargo run --release -- run config.toml
```

To let `faf-qq-bot` manage NapCatQQ for you:

```bash
# One-time setup: create data directories
cargo run --release -- setup config.toml

# Start NapCatQQ and the bot
cargo run --release -- start config.toml

# Stop NapCatQQ
cargo run --release -- stop config.toml

# Check status
cargo run --release -- status config.toml
```

## Commands

| Command | Description |
|---|---|
| `/summary` or `/s` | Summarize the recent conversation in the group. |
| `/status` | Show the number of buffered messages. |
| `/help` | Show available commands. |

## Configuration

See `config.example.toml` for all options.

## Deployment

For Alibaba Cloud or any Linux VPS:

1. Install NapCatQQ + NTQQ on the server.
2. Run NTQQ in a virtual display if needed (`Xvfb`).
3. Copy your `config.toml` to the server.
4. Run `faf-qq-bot` with a process manager like `systemd` or `tmux`.

## License

MIT
