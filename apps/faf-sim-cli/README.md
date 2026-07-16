# faf-sim-cli

Command-line interface for the FAF build-queue simulator and the single-task
build-time predictor. Predictions can come from either a trained neural network
or the exact analytical solver.

## Documentation

See the [`docs/`](docs/index.md) directory for detailed guides on each command.

## Quick start

All examples below run from the project root (`/home/zw/code/rust_programming/octopus`) using a release build for best performance.

```bash
# Simulate a plan from JSON (prints every tick, 30 s post-queue tail)
cargo run --release -p faf-sim-cli -- build passive tmp/faf-sim-examples/engineer-builds-factory.json

# Simulate a plan and print only the final result (no tail).
# With --tail-seconds 0 the CLI runs the simulation directly, at the same
# speed as the dataset generator, instead of using the real-time service.
cargo run --release -p faf-sim-cli -- build passive --tail-seconds 0 tmp/faf-sim-examples/engineer-builds-factory.json

# Generate training data (single-task plans)
cargo run --release -p faf-sim-cli -- dataset generate --samples 10000

# Inspect the completion-time distribution of the current dataset
cargo run --release -p faf-sim-cli -- dataset histogram

# Train a predictor with default parameters.
# Artifacts are written to data/build_prediction_artifacts/.
cargo run --release -p faf-sim-cli -- train \
  --dataset data/build_prediction_dataset.db \
  --output-dir data/build_prediction_artifacts

# Train with time-based loss weighting to up-weight fast plans
cargo run --release -p faf-sim-cli -- train \
  --dataset data/build_prediction_dataset.db \
  --output-dir data/build_prediction_artifacts \
  --epochs 100 \
  --batch-size 64 \
  --hidden-size 256 \
  --learning-rate 0.001 \
  --time-weight-power 0.3

# Predict completion time with the trained neural network.
# Requires trained artifacts in --model-dir (default: data/build_prediction_artifacts).
cargo run --release -p faf-sim-cli -- predict nn \
  --model-dir data/build_prediction_artifacts \
  --plan tmp/faf-sim-examples/engineer-builds-factory.json

# Predict completion time with the analytical solver.
# No trained model is required. The solver processes tasks sequentially and
# folds each completed target's economy contributions into the next task.
cargo run --release -p faf-sim-cli -- predict solver \
  --plan tmp/faf-sim-examples/engineer-builds-factory.json
```

Run `cargo run --release -p faf-sim-cli -- --help` for all commands and flags.

## Dataset generation

`dataset generate` samples from real FAF unit definitions by default. It loads
`plugins/faf-units/data/faf_units.json`, derives builder and target pools from
units that can build and units that have a build recipe, simulates each sampled
plan, and stores per-task features plus completion times in SQLite.

The current predictor is trained on **single-task plans only**: every generated
sample contains exactly one `BuildTask`.

```bash
# 10k samples using the default real unit database
cargo run --release -p faf-sim-cli -- dataset generate --samples 10000

# Use a different units JSON file
cargo run --release -p faf-sim-cli -- dataset generate \
  --samples 10000 \
  --units-file path/to/faf_units.json
```

You can inspect the resulting completion-time distribution with the
[`dataset histogram`](docs/02-dataset.md) command.

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

Each task is encoded as a **22-dimensional** feature vector:

- the initial economy snapshot the plan starts from,
- task-level aggregates (build power, costs, production, maintenance, storage).

The predictor is single-task, so cumulative contributions from earlier tasks are
not included.

## Training

`train` loads the SQLite dataset, normalizes features, and trains a small MLP
that predicts `log(completion_time)`. Trained artifacts (`config.json`,
`norm.json`, `model.mpk`) are written to `--output-dir` and are required by
`predict nn`.

```bash
cargo run --release -p faf-sim-cli -- train \
  --dataset data/build_prediction_dataset.db \
  --output-dir data/build_prediction_artifacts
```

## Time-weighted training loss

Randomly generated plans span a wide range of completion times, which can bias
the predictor. The `train` command supports `--time-weight-power` to weight each
sample by `raw_time^{-power}`:

- `0.0` (default) — standard unweighted MSE.
- `0.5` — moderate up-weighting of fast plans.
- `1.0` — strong up-weighting; fast plans have much more influence on gradients.

Start with `0.5` and adjust based on validation metrics.

## Predicting

`predict` estimates how long a plan will take. It has two backends, both
single-task:

### `predict nn`

Uses the MLP trained by the `train` command. Requires `--model-dir` with the
artifacts produced by training (`config.json`, `norm.json`, `model.mpk`).

```bash
cargo run --release -p faf-sim-cli -- predict nn \
  --model-dir data/build_prediction_artifacts \
  --plan tmp/faf-sim-examples/engineer-builds-factory.json
```

The model was trained on single-task plans, so only the first task in the plan
is used for inference.

### `predict solver`

Uses the exact analytical solver (`faf_sim::plan_completion_time`). No trained
model is required. The solver processes each task in the plan sequentially:
when a target finishes, its production, maintenance, and storage contributions
are added to the running economy before the next task's `start_after` delay is
applied.

```bash
cargo run --release -p faf-sim-cli -- predict solver \
  --plan tmp/faf-sim-examples/engineer-builds-factory.json
```

You can raise or lower the safety cap with `--max-time-seconds` (default 6000).
The output includes the final economy snapshot under the `economy` key and a
per-task breakdown under `tasks`.

### Input format

Both predict modes use the same `BuildQueue` JSON file as `build`. They derive
the initial economy snapshot from the file's `initial_eco` field, so no separate
`eco.json` is required. You can still pass one explicitly with `--eco` if you
want to override the plan's economy.
