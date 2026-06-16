# 5. Health, Status, and Diagnostics

## 5.1 What each command checks

| Command | Scope | Sends a test message? |
|---------|-------|----------------------|
| `status` | Infrastructure + basic application health | No |
| `health` | Full end-to-end check | Yes |
| `doctor` | Docker, binaries, configs, ports, handshake | No |

## 5.2 `status`

`status` checks:

- the supervisor daemon is running
- the SnowLuma container is running
- the OneBot WebSocket port is reachable
- `qqbot-core` is running
- the SnowLuma WebUI and noVNC ports are reachable
- the bot account is online
- the bot is a member of every configured allowed group

Because it does not send messages, you can run it as often as you like.

## 5.3 `health`

`health` is the strongest readiness signal. It:

1. Connects to the OneBot WebSocket.
2. Calls `get_login_info` and `get_status` to verify the QQ account is online.
3. Calls `get_group_member_info` for each allowed group to verify membership.
4. Sends a short, unique test message to the first allowed group.
5. Calls `get_group_msg_history` to confirm the message was delivered.

The test message posted to the group looks like:

```text
LLM:
  provider: https://api.moonshot.cn/v1/chat/completions
  model: moonshot-v1-8k
Tools loaded:
  1. summary -- Summarize recent group chat messages
Total: 1/1 tools available
Mention the bot with @<question> to chat.
check id: <uuid>
```

If `health` reports all checks green, the bot can both send and receive messages in the allowed group.

## 5.4 `doctor`

`doctor` is useful when `status` or `health` fails. It verifies:

- Docker daemon is reachable
- the SnowLuma image exists
- `qqbot-core` and plugin binaries are built
- config files exist
- the daemon/container/core processes are in expected states
- required ports are reachable
- the OneBot WebSocket handshake works

## 5.5 Common status meanings

### `[fail] Bot is not a member of allowed group ...`

Add the bot's QQ account to the group from a normal QQ client.

### `[warn] Bot is online and in the allowed group(s), but the end-to-end echo did not complete`

Only seen from `health`. The message may have been sent but not yet visible in history. Wait a few seconds and rerun.

### `[fail] qqbot-core binary not found`

Run:

```bash
cargo build -p qqbot-core
```
