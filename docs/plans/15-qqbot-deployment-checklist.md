# QQ Bot AliCloud ECS Deployment Checklist

A step-by-step manual for deploying the QQ bot to AliCloud ECS and managing it remotely.

---

## 1. Local prerequisites

- [ ] Local `data/qqbot-data/` is initialized:
  - `data/qqbot-data/config.toml`
  - `data/qqbot-data/groups/` (desired group profiles)
  - `data/qqbot-data/plugins/` (if any)
  - `data/snowluma-data/config/` (OneBot/WebUI configs)
- [ ] AliCloud account and a RAM user with ECS/VPC permissions are ready.
- [ ] `aliyun` CLI is installed locally.
- [ ] OpenSSH client (`ssh` / `scp`) is installed locally.

## 2. Configure AliCloud CLI

```bash
aliyun configure --profile default
```

- [ ] Enter your AccessKey ID.
- [ ] Enter your AccessKey Secret.
- [ ] Set default region (e.g. `cn-hangzhou`).
- [ ] Verify it works:

```bash
aliyun configure list
```

## 3. Configure deployment settings

Edit `data/qqbot-data/deploy.toml`:

- [ ] `region` and `zone` match your AliCloud account.
- [ ] `instance_type` is available in the chosen zone.
- [ ] `image_id` is a valid x64 image for the chosen region.
- [ ] `allowed_ssh_cidr` is set to **your current public IP/32** (not `0.0.0.0/0`).
- [ ] `key_pair_name` is acceptable (e.g. `qqbot-key`).
- [ ] `remote.user` is `qqbot` (recommended; script will create this user).
- [ ] `remote.install_dir` is `/opt/qqbot` (recommended).
- [ ] `remote.ssh_private_key` path is where you want the PEM key saved.

Example secure CIDR:

```toml
allowed_ssh_cidr = "203.0.113.42/32"
```

## 4. Build and deploy

Run the deploy command. This will provision AliCloud resources and install the bot.

```bash
cargo xtask qqbot deploy
```

The command will:

- [ ] Build `dist/qqbot-linux-x86_64.tar.gz`.
- [ ] Create or reuse VPC, VSwitch, security group, and SSH key pair on AliCloud.
- [ ] Create or start an ECS instance.
- [ ] Upload binaries, plugins, and local data to `/opt/qqbot`.
- [ ] Install Docker, create the `qqbot` user, and pull the SnowLuma image.
- [ ] Install and start the `qqbot.service` systemd unit.
- [ ] Run `qqbot doctor` and `qqbot status` remotely.

Wait until the command finishes and prints remote status / doctor output.

## 5. Verify the remote service

Run these commands from your local machine:

```bash
# Check service status
cargo xtask qqbot remote-status

# Check health
cargo xtask qqbot remote-health

# Run remote doctor
cargo xtask qqbot remote-doctor
```

- [ ] `remote-status` shows the supervisor is running.
- [ ] `remote-health` reports healthy.
- [ ] `remote-doctor` reports no critical issues.

## 6. Day-to-day remote management

```bash
# View recent logs
cargo xtask qqbot remote-logs core -n 100

# Start / stop / restart the service
cargo xtask qqbot remote-start
cargo xtask qqbot remote-stop
cargo xtask qqbot remote-restart
```

## 7. Destroy the deployment (optional)

If you want to tear everything down:

```bash
cargo xtask qqbot remote-destroy
```

- [ ] Confirm the prompt.
- [ ] The ECS instance will be deleted. VPC / VSwitch / security group / key pair are currently left as-is.

## 8. Local troubleshooting

If the local `snowluma` container becomes stuck and cannot be killed normally, recover with **root** privileges:

```bash
# Option A: kill the container process directly
sudo kill -9 $(docker inspect -f '{{.State.Pid}}' snowluma)

# Option B: restart Docker entirely
sudo systemctl restart docker
```

Then restart locally:

```bash
cargo xtask qqbot start
```

## 9. Security reminders

- [ ] Do not commit `data/qqbot-data/config.toml` or `data/qqbot-data/deploy.toml` to Git.
- [ ] Keep `allowed_ssh_cidr` restricted to your public IP.
- [ ] Protect the generated SSH private key (stored at `~/.ssh/qqbot-key.pem` by default).
- [ ] The Kimi API key and QQ account credentials live in `config.toml`, which is synced to the server.
