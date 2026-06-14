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

```bash
cargo build --release -p qqbot -p qqbot-core
cargo build --release -p summary --target wasm32-unknown-unknown
```

After this you have:

- `./target/release/qqbot` — supervisor CLI
- `./target/release/qqbot-core` — bot runtime
- `./target/wasm32-unknown-unknown/release/summary.wasm` — default plugin

## 2.3 Initialize

`init` writes config files, pulls the SnowLuma Docker image, copies the default plugin, and starts the daemon.

```bash
./target/release/qqbot init \
  --account 3462039501 \
  --kimi-key sk-xxxxxx \
  --group 925712027
```

Flags:

- `--account` — the bot's QQ number
- `--kimi-key` — Moonshot API key
- `--group` — allowed group ID (repeatable for multiple groups)
- `--data-dir` — defaults to `./data/qqbot-data`

## 2.4 Log in

After `init`, SnowLuma starts inside Docker. Open noVNC and scan the QQ QR code:

```bash
# URL and password
http://localhost:6081
# password: vncpasswd
```

In the noVNC desktop, scan the QR code with your phone's QQ app to log in the bot account.

## 2.5 Add the bot to the group

`--group` only configures a permission filter. You still need to add the bot QQ account as a member of the real QQ group manually from your phone or desktop QQ client.

## 2.6 Verify

```bash
./target/release/qqbot status
./target/release/qqbot health
```

`health` sends a short test message to the first allowed group and confirms it appears in the group's history. It is an explicit, user-triggered check and does not run on a schedule.
