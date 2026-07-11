# faf-sim-cli

CLI for the headless FAF build-queue simulator.

It reads a construction plan as JSON, runs the simulation, and emits events as
NDJSON. These events can be consumed by other tools, plotted, or piped into a
file.

## Simulate a construction plan

```sh
cargo run --bin faf-sim -- simulate path/to/plan.json --dt 0.1 --max-time 3600
```

- `plan.json` — a `BuildQueue` JSON object describing initial economy and build tasks.
- `--dt` — simulation step size in seconds (default `0.1`).
- `--max-time` — hard cap to prevent infinite simulation (default `3600`).

## Construction plan format

A plan is a JSON object with `initial_eco` and `tasks`:

```json
{
  "initial_eco": {
    "net_mass_income": 1.0,
    "net_energy_income": 20.0,
    "mass_storage": { "current": 650.0, "cap": 650.0 },
    "energy_storage": { "current": 4000.0, "cap": 4000.0 }
  },
  "tasks": [
    {
      "id": 1,
      "start_after": 0.0,
      "builders": [
        { "build_power": 10.0, "mass_cost": 0.0, "energy_cost": 0.0, "build_time": 0.0 }
      ],
      "target": {
        "build_power": 0.0,
        "mass_cost": 240.0,
        "energy_cost": 2100.0,
        "build_time": 300.0
      }
    }
  ]
}
```

- `initial_eco` — starting mass/energy income and storage.
- `tasks` — list of build tasks.
  - `id` — caller-defined identifier, echoed in events.
  - `start_after` — simulation time before the task may begin.
  - `builders` — units providing build power.
    - `build_power` — build rate of the builder.
  - `target` — unit being built.
    - `mass_cost`, `energy_cost`, `build_time` — unit build stats.

Optional fields on both `builders` and `target` (`mass_income`,
`energy_income`, `maintenance_energy`, `mass_storage`, `energy_storage`) are
used after a unit completes to affect the economy. They default to `0.0`.

## Example plan

A ready-to-run example lives at:

```
tmp/faf-sim-examples/engineer-builds-factory.json
```

Run it with:

```sh
cargo run --bin faf-sim -- simulate tmp/faf-sim-examples/engineer-builds-factory.json --dt 1.0 --max-time 1000
```

## Output

The CLI prints one event per line:

```json
{"TaskStarted":{"task_id":1,"time":0.0}}
{"Ticked":{"time":0.1,"mass_income":1.0,"energy_income":20.0,...}}
{"TaskCompleted":{"task_id":1,"time":30.0}}
"Finished"
```

Event types:

- `TaskStarted { task_id, time }`
- `Ticked(EcoSnapshot)` — economy state at this tick
- `TaskCompleted { task_id, time }`
- `Finished`

## Pipe to a file

```sh
cargo run --bin faf-sim -- simulate plan.json > events.ndjson
```
