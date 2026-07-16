# `predict` command

Estimate how long a plan will take. The command has two backends:

- `predict nn` — run inference with a trained MLP regression model.
- `predict solver` — run the exact analytical solver.

Both take the same `BuildQueue` JSON file as the [`build`](01-build.md) command
and derive the initial economy snapshot from the plan's `initial_eco` field.

## `predict nn`

Uses the model artifacts produced by [`train`](03-train.md):
`config.json`, `model.mpk`, and `norm.json`.

```bash
faf-sim predict nn --plan tmp/faf-sim-examples/engineer-builds-factory.json
```

| Flag | Default | Description |
|------|---------|-------------|
| `--plan` | required | Path to a JSON `BuildQueue` file. |
| `--eco` | omitted | Path to a JSON `EcoSnapshot` file. If omitted, the snapshot is derived from the plan's `initial_eco`. |
| `--model-dir` | `data/build_prediction_artifacts` | Directory containing `config.json`, `model.mpk`, and `norm.json`. |

The model was trained on single-task plans, so only the first task in the plan
is used for inference.

## `predict solver`

Uses `faf_sim::plan_completion_time` to compute the exact completion time. No
trained model is required. The solver processes each task in the plan
sequentially: when a target finishes, its production, maintenance, and storage
contributions are added to the running economy before the next task's
`start_after` delay is applied.

```bash
faf-sim predict solver --plan tmp/faf-sim-examples/engineer-builds-factory.json
```

| Flag | Default | Description |
|------|---------|-------------|
| `--plan` | required | Path to a JSON `BuildQueue` file. |
| `--eco` | omitted | Path to a JSON `EcoSnapshot` file. If omitted, the snapshot is derived from the plan's `initial_eco`. |
| `--max-time-seconds` | `6000` | Safety cap on how many seconds the solver may run. |

This mode is useful for validating model predictions or for predictions where
you do not have trained artifacts.

## Derived economy snapshot

When `--eco` is omitted, the CLI converts the plan's `initial_eco`
(`EconomyRuntimeState`) into an `EcoSnapshot`:

- `time` is set to `0.0`.
- Production, maintenance, and storage values are copied from the plan.
- `mass_drain`, `energy_drain`, `total_mass_spent`, and `total_energy_spent`
  default to `0.0`.

This matches the starting state the simulator would use when running the same
plan with `build`.

If you need full control, provide a standalone `--eco` file with this exact
shape:

```json
{
  "time": 0.0,
  "production_per_second_mass": 50.0,
  "production_per_second_energy": 101.0,
  "maintenance_consumption_per_second_energy": 0.0,
  "mass_drain": 0.0,
  "energy_drain": 0.0,
  "total_mass_spent": 0.0,
  "total_energy_spent": 0.0,
  "mass_storage": 650.0,
  "mass_storage_cap": 650.0,
  "energy_storage": 4000.0,
  "energy_storage_cap": 4000.0
}
```

## Output

Both backends print a single JSON object with whole-second resolution. The
`solver` backend also includes the final economy snapshot and a per-task
breakdown under `tasks`:

```json
{
  "predicted_time_seconds": 21,
  "economy": {
    "time": 21.0,
    "production_per_second_mass": 10.0,
    "production_per_second_energy": 60.0,
    "maintenance_consumption_per_second_energy": 0.0,
    "mass_storage": 900.0,
    "mass_storage_cap": 1000.0,
    "energy_storage": 2700.0,
    "energy_storage_cap": 5000.0
  },
  "tasks": [
    {
      "predicted_time_seconds": 11,
      "economy": { "time": 11.0, ... }
    },
    {
      "predicted_time_seconds": 21,
      "economy": { "time": 21.0, ... }
    }
  ]
}
```

`predicted_time_seconds` is the estimated wall-clock time to complete the plan.
The neural-network backend uses only the first task for inference, while the
solver backend handles multi-task plans sequentially.
