# `dataset generate` command

Generate a SQLite dataset of simulated build plans and their completion times for training the predictor.

## Usage

By default, builders and targets are sampled from the real FAF unit database:

```bash
faf-sim dataset generate --samples 10000 --output dataset.sqlite
```

To use a different units file, pass `--units-file`:

```bash
# Use an alternate FAF unit index
faf-sim dataset generate --units-file path/to/faf_units.json
```

The CLI always samples from real units.

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--samples` | `10000` | Number of random `(eco, plan)` samples to generate. |
| `--output` | `data/build_prediction_dataset.db` | Path to the output SQLite file. |
| `--max-builders-per-task` | `3` | Maximum number of builders assigned to a single task. |
| `--max-targets-per-task` | `5` | Maximum number of target units inside a single task. |
| `--units-file` | `plugins/faf-units/data/faf_units.json` | Path to the FAF units JSON file. Builders and targets are sampled from real unit definitions. |

## Inspecting the distribution

After generation you can print an ASCII histogram and summary statistics of the
completion times without regenerating the data:

```bash
faf-sim dataset histogram
faf-sim dataset histogram --dataset path/to/dataset.db
```

This uses the same `print_time_distribution` helper that the old
`--histogram` flag invoked automatically.

## Sampling pipeline

1. Load `faf-units` and split units into a **builder pool** (units with `build_power > 0`) and a **target pool** (units that have a build recipe).
2. Sample a single `BuildTask` (one builder group + one target group).
3. Build a realistic starting economy for that task using the ACU plus 0–3 T1 engineers, 0–3 T1 power generators, and 0–2 T1 mass extractors, with storage filled to capacity.
4. Run the exact `faf-sim` simulator on the single-task plan to get the ground-truth completion time.
5. Store the per-task feature vector and label in SQLite.

The same steps are exposed as a fluent Rust pipeline for custom callers:

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

## Output schema

The SQLite file contains a single table `samples`:

```sql
CREATE TABLE samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sequence_features TEXT NOT NULL, -- JSON array with one 22-dim feature vector
    target_time REAL NOT NULL        -- simulated completion time (capped at 6000 s)
);
```

Rows are inserted in batches and the file can be opened with any SQLite client for inspection. Use `dataset histogram` for a quick built-in view of the completion-time distribution.
