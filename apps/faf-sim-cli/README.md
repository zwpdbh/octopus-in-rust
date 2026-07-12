# faf-sim-cli

CLI client for the FAF build-queue simulator.

It reads a construction plan as JSON, connects to a `faf-db-server` simulation
endpoint over WebSocket, and emits the streamed events as NDJSON. These events
can be consumed by other tools, plotted, or piped into a file.

The server must be running before using the CLI. Start it with:

```sh
cargo run -p faf-db-server
```

## Simulate a construction plan

```sh
cargo run --bin faf-sim -- build /home/zw/code/rust_programming/octopus/tmp/faf-sim-examples/engineer-builds-factory.json --resolution 10
```

- `plan.json` — a `BuildQueue` JSON object describing initial economy and build tasks.
- `--url` (`-u`) — WebSocket URL of the simulation server (default `ws://localhost:8081/ws/simulate`).
- `--resolution` (`-r`) — simulation resolution in steps per second (default `10`).
- `--max-time` (`-m`) — optional hard cap in seconds. When omitted the simulation runs until the build queue is empty.

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
  - `start_after` — simulation time before the task may begin.
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
cargo run --bin faf-sim -- build tmp/faf-sim-examples/engineer-builds-factory.json --url ws://localhost:8081/ws/simulate --resolution 1 --max-time 1000
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
cargo run --bin faf-sim -- build plan.json > events.ndjson
```
