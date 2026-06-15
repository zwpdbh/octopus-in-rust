# QQ Bot Solution Tutorial

This tutorial explains how the Octopus `qqbot` package works and how to run, manage, and extend it.

## Contents

1. [What is qqbot?](02-overview.md)
2. [Build and first-time setup](03-setup.md)
3. [Command reference](04-commands.md)
4. [WASM plugin system](05-plugins.md)
5. [Health, status, and diagnostics](06-health-status.md)
6. [Troubleshooting](07-troubleshooting.md)

## Quick reference

```bash
# Build dependencies (do once)
cargo build -p qqbot-core
cargo build --release -p summary --target wasm32-unknown-unknown

# Initialize and start
cargo run --bin qqbot -- init \
  --account 3462039501 \
  --kimi-key sk-xxxxxx \
  --group 925712027
cargo run --bin qqbot -- start

# Verify
cargo run --bin qqbot -- status
cargo run --bin qqbot -- health

# Manage plugins
cargo run --bin qqbot -- plugin list
cargo run --bin qqbot -- plugin disable summary
cargo run --bin qqbot -- plugin enable summary
```
