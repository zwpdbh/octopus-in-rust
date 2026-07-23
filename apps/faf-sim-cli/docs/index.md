# faf-sim-cli documentation

`faf-sim-cli` is the command-line interface for the FAF build-queue simulator.

It can:

- Simulate a construction plan and emit raw NDJSON events.
- Predict completion time with the exact analytical solver.
- Compute a build order that reaches an eco target or builds a target unit.

## Commands

| # | Command | Purpose |
|---|---------|---------|
| 1 | [`build`](01-build.md) | Simulate a construction plan and emit events. |
| 2 | [`predict solver`](04-predict.md) | Predict completion time for a concrete plan. |
| 3 | [`schedule`](05-schedule.md) | Compute a build order for an eco or unit target. |

Run any command with `--help` for the full list of flags.
