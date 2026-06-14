# QQ Bot Solution Tutorial

This tutorial explains how the Octopus `qqbot` package works and how to run, manage, and extend it.

## Contents

1. [What is qqbot?](02-overview.md)
2. [Build and first-time setup](03-setup.md)
3. [Command reference](04-commands.md)
4. [Plugin management](05-plugins.md)
5. [Health, status, and diagnostics](06-health-status.md)
6. [Troubleshooting](07-troubleshooting.md)

## Quick reference

```bash
# Build everything
cargo build --release -p qqbot -p qqbot-core
cargo build --release -p summary --target wasm32-unknown-unknown

# Initialize and start
./target/release/qqbot init \
  --account 3462039501 \
  --kimi-key sk-xxxxxx \
  --group 925712027
./target/release/qqbot start

# Verify
./target/release/qqbot status
./target/release/qqbot health

# Manage plugins
./target/release/qqbot plugin list
./target/release/qqbot plugin disable summary
./target/release/qqbot plugin enable summary
```
