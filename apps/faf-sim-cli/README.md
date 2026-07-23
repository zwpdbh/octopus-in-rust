# faf-sim-cli

Command-line interface for the FAF build-queue simulator and build-order scheduler.

This README is a quick index. Detailed examples and command references live in
the [`docs/`](docs/index.md) directory.

## Commands

| # | Command | Purpose | Docs |
|---|---------|---------|------|
| 1 | `build` | Simulate a construction plan and emit NDJSON events. | [`docs/01-build.md`](docs/01-build.md) |
| 2 | `predict solver` | Predict completion time with the analytical solver. | [`docs/04-predict.md`](docs/04-predict.md) |
| 3 | `schedule` | Compute a build order for an eco or unit target. | [`docs/05-schedule.md`](docs/05-schedule.md) |

Run any command with `--help` for the full list of flags:

```bash
cargo run --release -p faf-sim-cli -- --help
```

## Quick example

Simulate a plan from JSON:

```bash
cargo run --release -p faf-sim-cli -- build passive tmp/faf-sim-examples/engineer-builds-factory.json
```
