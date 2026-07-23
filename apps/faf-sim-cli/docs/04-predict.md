# `predict solver`

Predict the completion time of a concrete build plan using the analytical solver.

The solver simulates the economy and build queue deterministically and reports
when each task finishes, along with the final economy state.

## Usage

```bash
cargo run --release -p faf-sim-cli -- predict solver --plan tmp/faf-sim-examples/engineer-builds-factory.json
```

## Arguments

- `--plan <PATH>` — JSON file with the build queue (`BuildQueue`).
- `--eco <PATH>` *(optional)* — JSON file with an explicit initial economy snapshot.
  If omitted, the snapshot is derived from the plan's `initial_eco` field.
- `--max-time-seconds <N>` *(optional)* — Safety cap on how many seconds the solver
  may run. Defaults to 6000.

## Output

NDJSON-style single JSON object:

```json
{
  "predicted_time_seconds": 123,
  "economy": { ... },
  "tasks": [
    { "predicted_time_seconds": 45, "economy": { ... } },
    ...
  ]
}
```
