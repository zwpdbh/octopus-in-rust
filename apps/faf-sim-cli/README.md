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

# Train with more expressive parameters for a larger dataset
cargo run --release -p faf-sim-cli -- train \
  --dataset data/build_prediction_dataset.db \
  --epochs 100 \
  --batch-size 64 \
  --hidden-size 256 \
  --learning-rate 0.001

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
// crates/faf-build-prediction/src/data/generator.rs ~line 127 — DatasetGenerator::pipeline
DatasetGenerator::new(GenerationConfig::default())
    .with_units_file(Path::new("plugins/faf-units/data/faf_units.json"))?
    .pipeline(Path::new("data/dataset.db"))?
    .create_schema()?
    .generate_samples()?
    .save_norm()?
    .finish()?;
```

See [`docs/02-dataset.md`](docs/02-dataset.md) for more details.

## `predict` input

`predict` uses the same `BuildQueue` JSON file as `build`. It derives the initial economy snapshot from the file's `initial_eco` field, so no separate `eco.json` is required.
