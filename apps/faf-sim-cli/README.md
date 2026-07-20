# faf-sim-cli

Command-line interface for the FAF build-queue simulator, build-time predictor,
and build-order scheduler.

This README is a quick index. Detailed examples and command references live in
the [`docs/`](docs/index.md) directory.

## Commands

| # | Command | Purpose | Docs |
|---|---------|---------|------|
| 1 | `build` | Simulate a construction plan and emit NDJSON events. | [`docs/01-build.md`](docs/01-build.md) |
| 2 | `dataset generate` | Generate a SQLite dataset of simulated plan completion times. | [`docs/02-dataset.md`](docs/02-dataset.md) |
| 3 | `train` | Train a neural network to predict completion time. | [`docs/03-train.md`](docs/03-train.md) |
| 4 | `predict` | Predict completion time with `nn` or the analytical `solver`. | [`docs/04-predict.md`](docs/04-predict.md) |
| 5 | `schedule` | Compute a build order for an eco or unit target. | [`docs/05-schedule.md`](docs/05-schedule.md) |

Run any command with `--help` for the full list of flags:

```bash
cargo run --release -p faf-sim-cli -- --help
```

## Quick example

Simulate a plan from JSON:

```bash
cargo run --release -p faf-sim-cli -- build passive tmp/faf-sim-examples/engineer-builds-factory.json
```
