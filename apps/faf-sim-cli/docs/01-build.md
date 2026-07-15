# `build` command

Simulate a construction plan from JSON and emit NDJSON events.

## Usage

```bash
faf-sim-cli build passive <queue.json>
faf-sim-cli build active  <queue.json>
```

- `passive`: the simulation auto-steps and emits events until the queue is finished.
- `active`: the simulation waits for manual `Advance` commands (useful for UI-driven stepping).

## Input format

The queue JSON must be a `BuildQueue` object with the following shape:

```json
{
  "initial_eco": {
    "production_per_second_mass": 50.0,
    "production_per_second_energy": 101.0,
    "maintenance_consumption_per_second_energy": 0.0,
    "mass_storage": {
      "current": 650.0,
      "cap": 650.0
    },
    "energy_storage": {
      "current": 4000.0,
      "cap": 4000.0
    }
  },
  "tasks": [
    {
      "id": 1,
      "start_after": 1.0,
      "builders": [
        {
          "build_power": 32.5,
          "mass_cost": 0.0,
          "energy_cost": 0.0,
          "build_time": 0.0,
          "production_per_second_mass": 0.0,
          "production_per_second_energy": 0.0,
          "maintenance_consumption_per_second_energy": 0.0,
          "mass_storage": 0.0,
          "energy_storage": 0.0,
          "unit_id": "UEL0309"
        }
      ],
      "targets": [
        {
          "build_power": 0.0,
          "mass_cost": 3240.0,
          "energy_cost": 57600.0,
          "build_time": 6824.0,
          "production_per_second_mass": 0.0,
          "production_per_second_energy": 2500.0,
          "maintenance_consumption_per_second_energy": 0.0,
          "mass_storage": 0.0,
          "energy_storage": 0.0,
          "unit_id": "UEB1301"
        }
      ]
    }
  ]
}
```

Fields:

- `initial_eco`: starting economy state.
  - `production_per_second_mass` / `production_per_second_energy`: gross income rates.
  - `maintenance_consumption_per_second_energy`: total upkeep energy drain from owned units.
  - `mass_storage` / `energy_storage`: objects with `current` amount and `cap` capacity.
- `tasks`: list of tasks to run in order.
  - `id`: caller-defined identifier echoed in start/complete events.
  - `start_after`: delay after the previous task finishes before this task may begin.
  - `builders`: list of units that contribute build power to this task.
  - `targets`: list of units to build sequentially.

Each `builders` / `targets` entry is a `UnitEcoStats` object describing build power, costs, production, storage, and an optional `unit_id`.

## Output format

Events are written as NDJSON lines:

```json
{"Ticked":{"time":1.0,"production_per_second_mass":50.0,"production_per_second_energy":101.0,"maintenance_consumption_per_second_energy":0.0,"mass_drain":0.0,"energy_drain":0.0,"total_mass_spent":0.0,"total_energy_spent":0.0,"mass_storage":650.0,"mass_storage_cap":650.0,"energy_storage":4000.0,"energy_storage_cap":4000.0}}
{"TaskStarted":{"task_id":1,"time":1.0}}
{"TaskCompleted":{"task_id":1,"time":42.0}}
"Finished"
```

Use `--format grouped` to receive `Ticked` events grouped into `rates`, `storage`, `totals`, and `derived`:

```bash
faf-sim-cli build passive --format grouped tmp/faf-sim-examples/engineer-builds-factory.json
```

## Post-queue tail and final result

By default the simulation keeps ticking for 30 seconds after the build queue is empty so you can observe the post-queue economy. Use `--tail-seconds` to change this:

```bash
# Keep ticking for 10 seconds after the queue is empty (default still prints every tick)
faf-sim-cli build passive --tail-seconds 10 tmp/faf-sim-examples/engineer-builds-factory.json

# Stop immediately when the queue is empty and print only the final result
faf-sim-cli build passive --tail-seconds 0 tmp/faf-sim-examples/engineer-builds-factory.json
```

When `--tail-seconds 0` is used, all intermediate events are suppressed and a single final `Ticked` event is printed containing the final economy snapshot and the exact completion time:

```json
{"Ticked":{"time":42.0,"production_per_second_mass":50.0,...}}
```

## Pipe to file

```bash
faf-sim-cli build passive tmp/faf-sim-examples/engineer-builds-factory.json > build.ndjson
```
