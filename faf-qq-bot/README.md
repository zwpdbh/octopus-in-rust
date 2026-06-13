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

## Docker

### Build the image

```bash
cd /path/to/octopus
docker build -f faf-qq-bot/Dockerfile -t faf-qq-bot:latest .
```

### Run with docker-compose (NapCatQQ companion container)

A `docker-compose.yml` is provided at the workspace root. It runs NapCatQQ and `faf-qq-bot` on the same Docker network.

#### 1. Prepare directories and config

```bash
cd /path/to/octopus
mkdir -p napcat/app/.config/QQ napcat/app/napcat/config
cp faf-qq-bot/config.example.toml faf-qq-bot/config.toml
# Edit faf-qq-bot/config.toml:
#   onebot.ws_url = "ws://napcat:3001"
#   onebot.access_token = ""   (or the token you set in NapCatQQ)
#   bot.allowed_groups = [YOUR_GROUP_ID]
#   llm.api_key = "YOUR_KIMI_KEY"
```

#### 2. Start NapCatQQ only and log in

```bash
docker compose up napcat -d
```

Open the NapCatQQ WebUI at `http://localhost:6099/webui`, find the token in `napcat/app/napcat/config/webui.json`, then:
- Log in your QQ account via QR code.
- Go to **Network Config** and enable **WebSocket Server** on `0.0.0.0:3001`.

#### 3. Start the bot

```bash
docker compose up faf-qq-bot -d
```

#### 4. Test

Send `/summary` in the allowed QQ group.

#### 5. View logs

```bash
docker compose logs -f faf-qq-bot
docker compose logs -f napcat
```

#### 6. Stop everything

```bash
docker compose down
```

## Deployment

For Alibaba Cloud or any Linux VPS:

1. Push the `faf-qq-bot` image to a registry (e.g. Alibaba Cloud Container Registry).
2. Run NapCatQQ + NTQQ on the server (either in a separate container or directly).
3. Run `faf-qq-bot` with your `config.toml` mounted into the container.

If you run NapCatQQ in a separate container, use Docker networking or the host IP so `faf-qq-bot` can reach the OneBot WebSocket.

## License

MIT
