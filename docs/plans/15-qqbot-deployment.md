# qqbot AliCloud ECS Deployment

This document describes how to deploy the QQ bot service to a single AliCloud ECS instance using the `aliyun` CLI and `cargo xtask`.

## Prerequisites

- AliCloud account and a RAM user with permission to manage ECS/VPC.
- Local installation of the [`aliyun` CLI](https://help.aliyun.com/product/29966.html) configured with `aliyun configure`.
- Local `ssh` / `scp` (OpenSSH client).
- The local `data/qqbot-data/` directory is initialized with `config.toml` and the desired group profiles/plugins.

## Configuration Templates

The `data/qqbot-data/` directory is ignored by Git, but tracked `_*.toml` templates show the expected file structure:

```bash
cp data/qqbot-data/_config.toml data/qqbot-data/config.toml
cp data/qqbot-data/_deploy.toml data/qqbot-data/deploy.toml
cp data/qqbot-data/groups/_example.toml data/qqbot-data/groups/123456789.toml
```

Edit the copied files and fill in your own values. Do not commit the copied files -- they contain credentials and account-specific settings.

## Configuration

Edit `data/qqbot-data/deploy.toml`:

```toml
[aliyun]
region = "cn-shanghai"
zone = "cn-shanghai-m"
instance_type = "ecs.e-c1m1.large"
# Use an image whose glibc matches your local build environment.
# The release binary is built locally and linked dynamically; Ubuntu 24.04
# (glibc 2.39) is a safe choice if you build on a recent Ubuntu/Debian host.
image_id = "ubuntu_24_04_x64_20G_alibase_20260522.vhd"
vpc_cidr = "192.168.0.0/16"
vswitch_cidr = "192.168.0.0/24"
key_pair_name = "qqbot-key"
# Use your own public IP/32 here for security.
allowed_ssh_cidr = "0.0.0.0/0"
# CIDR allowed to reach SnowLuma WebUI, noVNC, and VNC.
allowed_service_cidr = "0.0.0.0/0"
aliyun_profile = "default"
name = "octopus-qqbot"

[remote]
user = "qqbot"
install_dir = "/opt/qqbot"
ssh_private_key = "~/.ssh/qqbot-key.pem"
```

> **Important:** The `region` and `zone` in `deploy.toml` must match the region configured in your `aliyun` CLI profile. You can verify the profile region with `aliyun configure list`. If they differ, either re-run `aliyun configure` with the same region, or update `deploy.toml` (and the `image_id`, since image IDs are region-specific).
>
> The `image_id` OS must have a glibc version compatible with your local build environment, because the release binary is built locally and linked dynamically. If you see errors like `version 'GLIBC_2.xxx' not found`, switch to a newer image (e.g., Ubuntu 24.04) or build the binary directly on the server.
>
> `allowed_ssh_cidr` and `allowed_service_cidr` should be your current public IP/32. `allowed_service_cidr` controls access to SnowLuma WebUI (5099), noVNC (6081), and VNC (5900). If your public IP is dynamic, update these values (or the corresponding security group rules) before each session. Only use `0.0.0.0/0` temporarily and if you accept the increased exposure.
>
> The bootstrap script automatically configures a Docker Hub mirror so the SnowLuma image can be pulled from AliCloud China. If the mirror stops working, edit `scripts/qqbot-remote-setup.sh` and replace the mirror URL.
>
> `cargo xtask qqbot deploy` rewrites local-only paths in `config.toml` (such as `bot.plugin_dir` and `llm.token_file`) to their remote equivalents and syncs the credential file to the service user's home directory.

## Deploy

```bash
cargo xtask qqbot deploy
```

To start from a completely clean slate (delete the existing ECS instance and SSH key pair, then recreate them), use:

```bash
cargo xtask qqbot deploy --fresh
```

Use `--yes` to skip the confirmation prompt:

```bash
cargo xtask qqbot deploy --fresh --yes
```

This is useful when moving to a new development machine that does not have the old SSH private key.

> **Note:** A fresh deploy creates a new post-paid ECS instance. AliCloud usually requires at least **100 CNY** of available cash balance to create a post-paid instance; the deployer checks this before deleting the old instance to avoid leaving you with no instance and no way to create a new one.

This command:

1. Builds a release tarball (`dist/qqbot-linux-x86_64.tar.gz`).
2. Creates or reuses a VPC, VSwitch, security group, and SSH key pair on AliCloud.
3. Creates or starts an ECS instance.
4. Uploads binaries, plugins, and the local data directory to `/opt/qqbot` on the server.
5. Installs Docker, creates the `qqbot` user, pulls the SnowLuma image.
6. Installs and starts the `qqbot.service` systemd unit.
7. Runs `qqbot doctor` and `qqbot status` on the remote host.

## Remote Management

```bash
cargo xtask qqbot remote-status
cargo xtask qqbot remote-logs core -n 100
cargo xtask qqbot remote-restart
cargo xtask qqbot remote-stop
cargo xtask qqbot remote-start
cargo xtask qqbot remote-doctor
cargo xtask qqbot remote-health
```

## Manual QQ Login (One-Time)

SnowLuma manages the QQ account. After the first deploy you must log the QQ account in once through SnowLuma's WebUI or noVNC. Until this is done, the OneBot WebSocket handshake will fail and `qqbot-core` will not be able to receive or send group messages.

Open one of these URLs in a browser (replace `<server-ip>` with the public IP printed by `cargo xtask qqbot deploy`):

```text
http://<server-ip>:5099   # SnowLuma WebUI
http://<server-ip>:6081   # noVNC (password: vncpasswd)
```

1. Follow the on-screen QQ login flow and scan the QR code.
2. Wait until QQ shows as online inside SnowLuma.
3. Back on your local machine, run:

   ```bash
   cargo xtask qqbot remote-status
   cargo xtask qqbot remote-health
   ```

   `remote-status` should show the supervisor running and `remote-health` should report healthy once `qqbot-core` has connected to OneBot.

> **Security:** These ports are only reachable from `allowed_service_cidr`. Keep that CIDR restricted to your current public IP/32. The default VNC password is `vncpasswd`; change it inside SnowLuma if you plan to expose noVNC for extended periods.

## Start / Stop the ECS Instance to Save Fees

When the bot is not needed (e.g., overnight), stop the ECS instance to avoid compute charges. Disk and IP-related charges may still apply depending on your AliCloud configuration.

```bash
# Stop the instance (saves compute fees)
cargo xtask qqbot remote-stop-instance

# Start it again when needed
# The qqbot systemd service is enabled, so it will start automatically.
cargo xtask qqbot remote-start-instance

# Verify the service is back up
cargo xtask qqbot remote-status
```

> **Note:** `remote-stop` stops the qqbot systemd service but keeps the ECS running. Use `remote-stop-instance` to stop the entire ECS instance.

## Destroy

```bash
cargo xtask qqbot remote-destroy
```

You will be prompted before the ECS instance is deleted.

## Switching to Another Computer

The deployment is idempotent — the same ECS instance, VPC, security group, and key pair will be reused — but a few local files are **not tracked by Git**. Copy these to the new machine before running `cargo xtask qqbot deploy`:

1. **SSH private key:** `~/.ssh/qqbot-key.pem` (the key generated by the first deploy). Keep permissions `0600`.
2. **`data/qqbot-data/config.toml`** — bot account, groups, LLM settings, and credentials.
3. **`data/qqbot-data/deploy.toml`** — AliCloud settings. Update `allowed_service_cidr` to the new machine's public IP/32, or use `0.0.0.0/0` temporarily.
4. **`data/qqbot-data/groups/*.toml`** — per-group system prompts and enabled plugins.
5. **`data/snowluma-data/config/`** — OneBot/WebUI configuration (optional; defaults will be created if missing).
6. **Kimi credential file** referenced by `llm.token_file` in `config.toml` (currently `~/.kimi/credentials/kimi-code.json`).
7. **Build environment:** you must build the release binary on an `x86_64` Linux host (or a host whose glibc is compatible with the remote Ubuntu 24.04 image). Cross-compilation is not configured.

Alternatively, run a fresh deploy from the new machine:

```bash
cargo xtask qqbot deploy --fresh
```

This destroys the existing ECS instance and AliCloud key pair, generates a new key pair on the new machine, and creates a brand-new instance. You do **not** need the old `~/.ssh/qqbot-key.pem` if you use `--fresh`.

On the new machine you also need:

- The [`aliyun` CLI](https://help.aliyun.com/product/29966.html) installed and configured with a profile whose region matches `deploy.toml` (`cn-shanghai`).
- Rust toolchain (`cargo`) to build the release.
- OpenSSH client (`ssh`/`scp`).

Quick sanity check before deploying:

```bash
ls -la ~/.ssh/qqbot-key.pem
ls -la data/qqbot-data/config.toml data/qqbot-data/deploy.toml
cargo check -p xtask
aliyun configure list
```

## Security Notes

- Keep `allowed_ssh_cidr` and `allowed_service_cidr` restricted to your public IP/32 whenever possible.
- If your public IP is dynamic, update the security group rules (or `deploy.toml`) before each session. Only use `0.0.0.0/0` temporarily and if you accept the increased exposure.
- The Kimi API key and QQ account live in `data/qqbot-data/config.toml`, which is synced to the server. Do not commit this file.
- The SSH private key generated by `cargo xtask qqbot deploy` is saved to the path configured in `ssh_private_key` with `0600` permissions.

## Implementation Details

- `xtask/src/deploy/` contains the deployment logic.
- `scripts/qqbot-remote-setup.sh` bootstraps the server environment.
- `scripts/qqbot.service` is the systemd unit template.
- `qqbot start --no-daemon` runs the supervisor in the foreground for systemd.
- `apps/qqbot/src/service.rs` locates `qqbot-core` next to `qqbot` when not running from `target/`.
