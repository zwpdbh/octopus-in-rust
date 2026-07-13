# faf-sim-cli

CLI for the FAF build-queue simulator.

It reads a construction plan as JSON, runs the simulation locally through
`faf-sim-service`, and emits events as NDJSON. These events can be consumed by
other tools, plotted, or piped into a file.

## Simulate a construction plan

The `build` command has two subcommands that select the simulation mode:

- `build active` — manual advance mode. The simulation starts and waits for
  external `Advance` commands (useful when driven by the WebSocket server or the
  service API). The CLI itself does not auto-step.
- `build passive` — auto-play mode. The simulation steps automatically using the
  configured tick interval.

### Passive mode

```sh
cargo run --bin faf-sim -- build passive \
  tmp/faf-sim-examples/engineer-builds-factory.json \
  --dt-seconds 1 \
  --max-time-seconds 1000 \
  --tick-interval-ms 50
```

- `<QUEUE>` — a `BuildQueue` JSON object describing initial economy and build tasks.
- `--dt-seconds` (`-d`) — simulation step size in seconds. Must be an integer `>= 1` (default `1`).
- `--max-time-seconds` (`-m`) — optional hard cap in seconds. When omitted the simulation runs until the build queue is empty.
- `--tick-interval-ms` — real-world delay between simulation steps in milliseconds (default `50`). Only available in passive mode.

### Active mode

```sh
cargo run --bin faf-sim -- build active \
  tmp/faf-sim-examples/engineer-builds-factory.json \
  --dt-seconds 1 \
  --max-time-seconds 1000
```

Active mode accepts the same `--dt-seconds` and `--max-time-seconds` options as
passive mode, but it does **not** accept `--tick-interval-ms`. For convenience,
the CLI drives active mode itself by sending `Advance` commands in a tight loop
until the simulation finishes.

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
        { "build_power": 10.0 }
      ],
      "target": {
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
  - `start_after` — delay after the previous task finishes before this task may begin. For the first task this is a delay relative to simulation start (time 0).
  - `builders` — units providing build power. Only `build_power` is relevant; other fields may be omitted and default to `0.0`.
  - `target` — unit being built. Only `mass_cost`, `energy_cost`, and `build_time` are relevant; `build_power` may be omitted and defaults to `0.0`.

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
cargo run --bin faf-sim -- build passive \
  tmp/faf-sim-examples/engineer-builds-factory.json \
  --dt-seconds 1 \
  --max-time-seconds 1000 \
  --tick-interval-ms 50
```

## Output

The CLI prints one event per line:

```json
{"TaskStarted":{"task_id":1,"time":0.0}}
{"Ticked":{"time":1.0,"mass_income":1.0,"energy_income":20.0,...}}
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
cargo run --bin faf-sim -- build passive plan.json > events.ndjson
```
