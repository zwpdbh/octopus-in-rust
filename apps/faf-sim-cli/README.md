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

## `predict` input

`predict` uses the same `BuildQueue` JSON file as `build`. It derives the initial economy snapshot from the file's `initial_eco` field, so no separate `eco.json` is required.
