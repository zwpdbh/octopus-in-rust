# faf-sim-cli

Command-line interface for the FAF build-queue simulator.

## Documentation

See the [`docs/`](docs/index.md) directory for detailed guides on each command.

## Quick start

All examples below run from the project root (`/home/zw/code/rust_programming/octopus`) using a release build for best performance.

```bash
# Simulate a plan from JSON
cargo run --release -p faf-sim-cli -- build passive tmp/faf-sim-examples/engineer-builds-factory.json

# Generate training data
cargo run --release -p faf-sim-cli -- dataset generate --samples 10000

# Train a predictor with default parameters
cargo run --release -p faf-sim-cli -- train --dataset data/build_prediction_dataset.db

# Train with time-based loss weighting to up-weight fast/practical plans
cargo run --release -p faf-sim-cli -- train \
  --dataset data/build_prediction_dataset.db \
  --epochs 50 \
  --batch-size 64 \
  --hidden-size 256 \
  --learning-rate 0.001 \
  --time-weight-power 0.3

# Predict completion time for a plan (eco snapshot is derived from the plan file)
cargo run --release -p faf-sim-cli -- predict --plan tmp/faf-sim-examples/engineer-builds-factory.json
```

Run `cargo run --release -p faf-sim-cli -- --help` for all commands and flags.

## Dataset generation

`dataset generate` samples from real FAF unit definitions by default. It loads
`plugins/faf-units/data/faf_units.json`, derives builder and target pools from
units that can build and units that have a build recipe, simulates each sampled
plan, and stores per-task sequence features plus labels in SQLite.

```bash
# 10k samples using the default real unit database
cargo run --release -p faf-sim-cli -- dataset generate --samples 10000

# Use a different units JSON file
cargo run --release -p faf-sim-cli -- dataset generate \
  --samples 10000 \
  --units-file path/to/faf_units.json
```

The same generator is exposed as a fluent Rust pipeline:

```rust
// crates/faf-build-prediction/src/data/generator.rs ~line 114 — DatasetGenerator::pipeline
DatasetGenerator::new(
    GenerationConfig::default(),
    Path::new("plugins/faf-units/data/faf_units.json"),
)?
.pipeline(Path::new("data/dataset.db"))?
.create_schema()?
.generate_samples()?
.save_norm()?
.finish()?;
```

See [`docs/02-dataset.md`](docs/02-dataset.md) for more details.

## Feature vector

Each task is encoded as a 27-dimensional feature vector:

- the initial economy snapshot the plan starts from,
- task-level aggregates (build power, costs, production, maintenance, storage),
- cumulative economy contributions from all earlier tasks in the plan.

The cumulative deltas let the model see how the economy evolves as earlier tasks
complete, e.g. a mass extractor built in Task 0 increases mass income available
to Task 1.

## Time-weighted training loss

Randomly generated plans are mostly slow / "not practical", which can bias the
predictor toward overestimating completion times for fast plans. The `train`
command supports `--time-weight-power` to weight each sample by
`raw_time^{-power}`:

- `0.0` (default) — standard unweighted MSE.
- `0.5` — moderate up-weighting of fast plans.
- `1.0` — strong up-weighting; fast plans have much more influence on gradients.

Start with `0.5` and increase if predictions for fast plans are still too high.

## `predict` input

`predict` uses the same `BuildQueue` JSON file as `build`. It derives the initial economy snapshot from the file's `initial_eco` field, so no separate `eco.json` is required.
