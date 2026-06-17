# 2. Build and First-Time Setup

## 2.1 Prerequisites

- Linux host
- Docker installed and your user can run `docker`
- Rust toolchain with the `wasm32-unknown-unknown` target
- One QQ account for the bot
- A Moonshot/Kimi API key (for the `summary` plugin)

Install the WASM target if you do not have it:

```bash
rustup target add wasm32-unknown-unknown
```

## 2.2 Build

For local development you only need to build `qqbot-core` and the plugins. `cargo run --bin qqbot` will build the supervisor itself automatically.

```bash
cargo build -p qqbot-core
cargo build --release -p faf-units-plugin --target wasm32-unknown-unknown
```

After this you have:

- `target/debug/qqbot` — supervisor CLI (built on demand by `cargo run`)
- `target/debug/qqbot-core` — bot runtime
- `target/wasm32-unknown-unknown/release/faf_units_plugin.wasm` — default plugin

## 2.3 Initialize

`init` writes config files, pulls the SnowLuma Docker image, copies the default plugin, starts SnowLuma, and prints a step-by-step checklist. On a fresh SnowLuma start it also prints the WebUI initial password.

```bash
cargo run --bin qqbot -- init \
  --account 3462039501 \
  --kimi-key sk-xxxxxx \
  --group 925712027
```

Flags:

- `--account` — the bot's QQ number
- `--kimi-key` — Moonshot API key
- `--group` — allowed group ID (repeatable for multiple groups)
- `--data-dir` — defaults to `./data/qqbot-data`
- `--ws-port` — OneBot WebSocket port, defaults to `3001`
- `--webui-port` — SnowLuma WebUI port, defaults to `5099`
- `--reset-webui-password` — reset the SnowLuma WebUI admin password and print the new one-time password

### Using a `.env` file

Instead of passing secrets on the command line, create a `.env` file in the
working directory:

```bash
QQBOT_ACCOUNT=3462039501
QQBOT_KIMI_KEY=sk-xxxxxx
QQBOT_GROUP=925712027
# optional:
# QQBOT_WS_PORT=3001
# QQBOT_WEBUI_PORT=5099
```

Then run:

```bash
cargo run --bin qqbot -- init
```

`init` loads `.env` automatically. Environment variables use the same names and
override any command-line flags you do provide.

## 2.4 Log in

After `init`, SnowLuma is already running inside Docker. The `init` output shows:

- noVNC URL: `http://localhost:6081` (password: `vncpasswd`)
- SnowLuma WebUI username: `admin` and the one-time password (only on the first fresh start, or when `--reset-webui-password` is used)

The WebUI password is one-time: use it for the first login, then change it in the WebUI. If you forget it, run `init` again with `--reset-webui-password`.

Open noVNC and scan the QQ QR code:

```bash
http://localhost:6081
# VNC password: vncpasswd
```

In the noVNC desktop, scan the QR code with your phone's QQ app to log in the bot account.

## 2.5 Add the bot to the group

`--group` only configures a permission filter. You still need to add the bot QQ account as a member of the real QQ group manually from your phone or desktop QQ client.

## 2.6 Verify

```bash
cargo run --bin qqbot -- status
cargo run --bin qqbot -- health
```

`health` sends a short test message to the first allowed group and confirms it appears in the group's history. It is an explicit, user-triggered check and does not run on a schedule.

## 2.7 Test the LLM

You can exercise the LLM configuration without running the full bot loop:

```bash
# Verify the API key and that the configured model exists
cargo run --bin qqbot -- llm test

# Send a single prompt and print the reply
cargo run --bin qqbot -- llm ask "summarize this"

# Stream a prompt and print chunks as they arrive, showing the lifecycle
cargo run --bin qqbot -- llm stream "hello, what is 2+2?"
```

`stream` is useful for watching the raw LLM behavior: it prints `[send]`,
`[headers]`, `[streaming]`, and `[done]` markers while flushing each token as
it arrives.
