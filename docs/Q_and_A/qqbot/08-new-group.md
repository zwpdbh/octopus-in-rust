# 7. Onboarding qqbot to a New QQ Group

This guide adds the bot to an additional QQ group after the initial setup.

## 7.1 What changes when adding a group

Three things must be updated:

1. **`data/qqbot-data/config.toml`** — add the new group ID to `bot.allowed_groups`. Optionally add a group-specific nickname to `bot.bot_aliases`.
2. **`data/qqbot-data/groups/<group_id>.toml`** — create a per-group config file with the system prompt and plugins for that group.
3. **The actual QQ group** — add the bot's QQ account as a member from your phone/desktop QQ client.

> **Note:** `bot.bot_aliases` is global. If you add an alias like `"FatBot"`, the bot will respond to plain-text `@FatBot` in **all** allowed groups. Per-group aliases are not supported by the current configuration schema. Native QQ `@` mentions work without any aliases.

## 7.2 Example: add group `136430130`

Suppose the bot's nickname in the new group should be `FatBot`.

### Step 1 — Update `config.toml`

Edit `data/qqbot-data/config.toml`:

```toml
[bot]
# Add the new group ID to the allow-list.
allowed_groups = [925712027, 136430130]
```

> **Note:** `bot_aliases` is only needed if you want the bot to respond to plain-text mentions like `@FatBot`. If users use QQ's native `@` feature (selecting the bot from the member list), the bot is identified by `bot_qq` and no alias is required.

### Step 2 — Create the group config file

Create `data/qqbot-data/groups/136430130.toml`:

```toml
# Per-group config for QQ group 136430130.
system_prompt = "You are FatBot, a helpful assistant for this QQ group."
enabled_plugins = ["faf_units_plugin"]

# Seconds between progress updates while the bot is working on a long answer.
# The message format is fixed to "Still checking... (tools: [<tool names>])".
# Default: 30.
progress_interval_secs = 30
```

Tailor `system_prompt` and `enabled_plugins` to the group's purpose. During slow, tool-backed answers the bot posts a short periodic progress message instead of flooding the chat with per-tool updates.

### Step 3 — Add the bot to the real QQ group

From your phone or desktop QQ client, add the bot's QQ account (`bot_qq` from `config.toml`) as a member of group `136430130`.

### Step 4 — Deploy the new config to the server

If you are running locally:

```bash
cargo run --bin qqbot -- restart
```

If you deployed to AliCloud ECS:

```bash
cargo xtask qqbot deploy
```

This syncs the updated `config.toml` and the new group file to `/opt/qqbot/data/qqbot-data/` and restarts the service.

### Step 5 — Verify

```bash
cargo xtask qqbot remote-status
cargo xtask qqbot remote-health --group 136430130
```

You should see the new group listed in the status output. The `--group` flag targets the end-to-end echo check at the new group instead of the first allowed group.

## 7.3 Quick reference

```bash
# 1. Edit config
data/qqbot-data/config.toml

# 2. Create group profile
cp data/qqbot-data/groups/_example.toml data/qqbot-data/groups/136430130.toml

# 3. Add bot to the real QQ group manually

# 4. Deploy / restart
#   Local:
cargo run --bin qqbot -- restart
#   Remote (AliCloud ECS):
cargo xtask qqbot deploy

# 5. Verify
cargo xtask qqbot remote-status
cargo xtask qqbot remote-health --group 136430130
```
