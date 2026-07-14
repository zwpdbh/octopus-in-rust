# `predict` command

Run inference with a trained LSTM sequence model to estimate how long a plan will take and whether it is practical.

## Usage

`predict` takes the same `BuildQueue` JSON file as the [`build`](01-build.md) command. The initial economy snapshot is derived from the plan's `initial_eco` field, so no separate eco file is needed:

```bash
faf-sim-cli predict --plan tmp/faf-sim-examples/engineer-builds-factory.json
```

If you want to override the derived snapshot, pass `--eco` with a standalone `EcoSnapshot` JSON file.

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--plan` | required | Path to a JSON `BuildQueue` file. |
| `--eco` | omitted | Path to a JSON `EcoSnapshot` file. If omitted, the snapshot is derived from the plan's `initial_eco`. |
| `--model-dir` | `data/build_prediction_artifacts` | Directory containing `config.json`, `model`, and `norm.json`. |
| `--practical-threshold-seconds` | `600` | Time limit used to decide `is_practical`. |

## Derived economy snapshot

When `--eco` is omitted, the CLI converts the plan's `initial_eco` (`EconomyRuntimeState`) into an `EcoSnapshot`:

- `time` is set to `0.0`.
- Production, maintenance, and storage values are copied from the plan.
- `mass_drain`, `energy_drain`, `total_mass_spent`, and `total_energy_spent` default to `0.0`.

This matches the starting state the simulator would use when running the same plan with `build`.

If you need full control, provide a standalone `--eco` file with this exact shape:

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

The result is printed as a single JSON object with whole-second resolution:

```json
{
  "is_practical": true,
  "predicted_time_seconds": 157
}
```

- `is_practical`: `true` when `predicted_time_seconds` is less than or equal to the threshold.
- `predicted_time_seconds`: model estimate of total wall-clock time to complete the plan.
