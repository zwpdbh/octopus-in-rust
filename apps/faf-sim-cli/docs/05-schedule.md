# `schedule` command

Compute a build order that reaches an eco target or builds a target unit. The
default algorithm is the greedy scheduler from `faf-build-scheduler`.

## `schedule eco`

Find the fastest way to reach a target mass income.

```bash
# No input file: start from a single ACU with the default economy and target
# 500 mass income per second.
cargo run --release -p faf-sim-cli -- schedule eco

# Override the mass target.
cargo run --release -p faf-sim-cli -- schedule eco --target-mass-production 70

# Provide an input file and write the result to a file.
cargo run --release -p faf-sim-cli -- schedule eco \
  /tmp/eco_input.json \
  -o /tmp/schedule_queue.json

# Override the target from the input file.
cargo run --release -p faf-sim-cli -- schedule eco \
  /tmp/eco_input.json \
  --target-mass-production 100 \
  -o /tmp/schedule_queue.json
```

The input file is a JSON `EcoScheduleInput`:

```json
{
  "initial_eco": {
    "time": 0.0,
    "production_per_second_mass": 5.0,
    "production_per_second_energy": 50.0,
    "maintenance_consumption_per_second_energy": 0.0,
    "mass_drain": 0.0,
    "energy_drain": 0.0,
    "total_mass_spent": 0.0,
    "total_energy_spent": 0.0,
    "mass_storage": 2000.0,
    "mass_storage_cap": 2000.0,
    "energy_storage": 4000.0,
    "energy_storage_cap": 4000.0
  },
  "initial_inventory": ["Commander"],
  "target_mass_production": 70.0
}
```

`initial_inventory` is optional and defaults to `["Commander"]`.

## `schedule unit`

Find the fastest way to build a target unit.

```bash
# Provide the target directly on the command line.
cargo run --release -p faf-sim-cli -- schedule unit --target Engineer(T1)

# Provide an input file and override the target.
cargo run --release -p faf-sim-cli -- schedule unit \
  /tmp/unit_input.json \
  --target Engineer(T1) \
  -o /tmp/schedule_queue.json
```

The input file is a JSON `UnitScheduleInput`:

```json
{
  "initial_eco": { ... },
  "initial_inventory": ["Commander"],
  "target": "Engineer(T1)"
}
```

When no input file and no `--target` are given, the default target is the UEF
Novax Center (`XEB2402`).
