# faf-sim-cli documentation

`faf-sim-cli` is the command-line interface for the FAF build-queue simulator.

It can:

- Simulate a construction plan and emit raw NDJSON events.
- Generate labeled training data for the build-time predictor.
- Train a neural network to predict how long a plan will take.
- Run inference with a trained model.

## Commands

| # | Command | Purpose |
|---|---------|---------|
| 1 | [`build`](01-build.md) | Simulate a construction plan and emit events. |
| 2 | [`dataset generate`](02-dataset.md) | Generate a SQLite dataset of simulated plan completion times. |
| 3 | [`train`](03-train.md) | Train the build-time prediction model. |
| 4 | [`predict`](04-predict.md) | Predict completion time for a concrete plan. |

Run any command with `--help` for the full list of flags.
