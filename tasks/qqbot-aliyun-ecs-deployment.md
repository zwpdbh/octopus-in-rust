# Task: qqbot-aliyun-ecs-deployment

## Goal

Add `cargo xtask qqbot deploy` and remote-management commands to provision an AliCloud ECS instance, install the qqbot release, and manage it via SSH/systemd.

## Background

The user confirmed AliCloud as the deployment target because its `aliyun` CLI provides the best command-line infrastructure support for the QQ bot service. The existing `xtask` runner already handles local build/start/status/logs; this task extends it to remote deployment.

## Scope

### In Scope

- `data/qqbot-data/deploy.toml` configuration file with AliCloud and remote settings.
- `xtask/src/deploy/` module: config parsing, `aliyun` CLI wrappers, SSH helpers, provisioning, and remote install/upgrade.
- `cargo xtask qqbot deploy` — build release, create/reuse ECS/VPC/VSwitch/SG/KeyPair, upload, install systemd, start.
- `cargo xtask qqbot remote-status/logs/restart/stop/start/doctor/health` — SSH wrappers.
- `cargo xtask qqbot remote-destroy` — tear down the ECS instance.
- `scripts/qqbot.service` systemd unit template.
- `scripts/qqbot-remote-setup.sh` remote environment bootstrap.
- Fix `apps/qqbot/src/service.rs` to find `qqbot-core` next to `qqbot` when not running from `target/`.
- Add `--no-daemon` option to `qqbot start` for systemd `Type=simple`.
- Clean up leftover `aliyun-cli-linux-latest-amd64.tgz`.

### Out of Scope

- AliCloud ROS/Terraform templates (can be added later).
- Automatic SnowLuma QQ login / QR code handling (still manual).
- Monitoring/alerting beyond `qqbot doctor`.

## Acceptance Criteria

- [ ] `cargo xtask qqbot deploy` builds a release tarball and provisions an ECS instance idempotently.
- [ ] `cargo xtask qqbot remote-status` shows the server's `qqbot status` output.
- [ ] `cargo xtask qqbot remote-restart` restarts the remote service.
- [ ] `cargo xtask qqbot remote-destroy` deletes the ECS instance after confirmation.
- [ ] `cargo run --bin qqbot -- start --no-daemon -d ./data/qqbot-data` runs the supervisor in the foreground.
- [ ] `cargo check --workspace` passes.
- [ ] `cargo test --workspace` passes.
- [ ] Docs updated with deployment steps.

## Implementation Notes

- Use local `ssh`/`scp` binaries rather than pulling in a Rust SSH crate to keep dependencies minimal.
- Use `aliyun --output json` and parse with `serde_json`.
- Tag resources with `Project=octopus-qqbot` and `Name=qqbot-<hostname>` for idempotent lookups.
- The local `data/qqbot-data/` directory (config, groups, plugins) is synced to the remote install directory.

## Completed Steps

- [x] Provider decision: AliCloud.
- [x] Task tracking file created.
- [x] Deploy config (`data/qqbot-data/deploy.toml`) and xtask deploy module implemented.
- [x] Remote scripts (`scripts/qqbot-remote-setup.sh`) and systemd unit (`scripts/qqbot.service`) added.
- [x] Supervisor fixes for installed layout and `--no-daemon`.
- [x] xtask command surface extended (`deploy`, `remote-*`, `remote-destroy`).
- [x] Local validation passed (`cargo check --workspace`, `cargo test --workspace`, `cargo clippy -p xtask -p qqbot --no-deps -- -D warnings`).
- [x] Docs updated (`docs/plans/15-qqbot-deployment.md`, `docs/plans/00-index.md`, `STATUS.md`).
- [x] Removed assumption that `aliyun` CLI supports `--output json`; it defaults to JSON.

## Notes / Blockers

- The local `snowluma` Docker container entered a stuck/unrecoverable state during the foreground `--no-daemon` smoke test (QQ process crashed with a GPU error). It currently cannot be killed without root. Recover with `sudo kill -9 $(docker inspect -f '{{.State.Pid}}' snowluma)` or `sudo systemctl restart docker`, then run `cargo xtask qqbot start`.
- A real AliCloud deployment test is still pending and requires a configured `aliyun` CLI profile.

## Decisions Made

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-06-17 | Use direct `aliyun` CLI calls instead of ROS/Terraform | Matches user's CLI-first requirement and keeps the resource lifecycle simple. |
| 2026-06-17 | Sync local `data/qqbot-data/` to remote `/opt/qqbot/data/qqbot-data/` | Preserves existing config.toml, group profiles, and plugins without running `qqbot init` remotely. |
| 2026-06-17 | Use `ssh`/`scp` binaries from xtask | Minimizes dependencies; no Rust async SSH library needed. |
