# faf-sim-cli documentation

`faf-sim-cli` is the command-line interface for the FAF build-queue simulator.

It can:

- Simulate a construction plan and emit raw NDJSON events.
- Generate labeled training data for the build-time predictor.
- Train a neural network to predict how long a plan will take.
- Predict completion time with either the trained neural network or the exact
  analytical solver.
- Compute a build order that reaches an eco target or builds a target unit.

## Commands

| # | Command | Purpose |
|---|---------|---------|
| 1 | [`build`](01-build.md) | Simulate a construction plan and emit events. |
| 2 | [`dataset generate`](02-dataset.md) | Generate a SQLite dataset of simulated plan completion times. |
| 3 | [`train`](03-train.md) | Train the build-time prediction model. |
| 4 | [`predict`](04-predict.md) | Predict completion time for a concrete plan (`nn` or `solver`). |
| 5 | [`schedule`](05-schedule.md) | Compute a build order for an eco or unit target. |

Run any command with `--help` for the full list of flags.
