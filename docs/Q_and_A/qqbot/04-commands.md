# 3. Command Reference

All `qqbot` commands accept `--data-dir <path>` to target a non-default data directory. Paths are resolved relative to the `qqbot` executable's project root, so you can run the binary from anywhere.

## 3.1 Lifecycle commands

### `init`

One-time setup. Writes configs, pulls the SnowLuma image, copies the default plugin, and starts the daemon.

```bash
cargo run --bin qqbot -- init \
  --account 3462039501 \
  --kimi-key sk-xxxxxx \
  --group 925712027
```

### `start`

Start the daemon in the background.

```bash
cargo run --bin qqbot -- start
```

### `stop`

Stop the daemon, `qqbot-core`, and the SnowLuma container.

```bash
cargo run --bin qqbot -- stop
```

### `restart`

Stop and start the daemon.

```bash
cargo run --bin qqbot -- restart
```

### `reset`

Stop everything and remove the QQ session data. Use this when you need to re-scan the QR code or start fresh.

```bash
cargo run --bin qqbot -- reset
```

## 3.2 Observability commands

### `status`

Print an infrastructure checklist and an application-level health summary. This command does **not** send test messages.

```bash
cargo run --bin qqbot -- status
```

Example output:

```text
[ok] qqbot daemon running (pid 72721)
[ok] SnowLuma container running
[ok] OneBot WebSocket reachable (ws://127.0.0.1:3001)
[ok] qqbot-core process running
[ok] SnowLuma WebUI port 5099 reachable
[ok] noVNC port 6081 reachable
[ok] Bot is online and in the allowed group(s)
       Logged in as zw112233 (3462039501)

Status: all systems go.
```

### `health`

Perform an explicit end-to-end health check. This command queries the OneBot API, checks group membership, sends a short test message to the allowed group, and confirms it appears in group history.

```bash
cargo run --bin qqbot -- health
```

### `doctor`

Run infrastructure diagnostics: Docker, SnowLuma image, binaries, configs, ports, and WebSocket handshake.

```bash
cargo run --bin qqbot -- doctor
```

### `logs`

Tail recent logs.

```bash
cargo run --bin qqbot -- logs core -n 50
cargo run --bin qqbot -- logs snowluma -n 50
cargo run --bin qqbot -- logs supervisor -n 50
```

## 3.3 Plugin commands

See [Plugin management](05-plugins.md) for details.

```bash
cargo run --bin qqbot -- plugin list
cargo run --bin qqbot -- plugin enable summary
cargo run --bin qqbot -- plugin disable summary
cargo run --bin qqbot -- plugin reload
```
