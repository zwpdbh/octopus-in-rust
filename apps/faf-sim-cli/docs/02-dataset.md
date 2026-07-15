# `dataset generate` command

Generate a SQLite dataset of simulated build plans and their completion times for training the predictor.

## Usage

By default, builders and targets are sampled from the real FAF unit database:

```bash
faf-sim-cli dataset generate --samples 10000 --output dataset.sqlite
```

To use a different units file, pass `--units-file`:

```bash
# Use an alternate FAF unit index
faf-sim-cli dataset generate --units-file path/to/faf_units.json
```

The CLI always samples from real units.

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--samples` | `10000` | Number of random `(eco, plan)` samples to generate. |
| `--output` | `data/build_prediction_dataset.db` | Path to the output SQLite file. |
| `--time-limit-seconds` | `600` | Practical threshold; slower plans are labeled as not practical. |
| `--max-tasks` | `5` | Maximum number of tasks in a generated plan. |
| `--max-builders-per-task` | `3` | Maximum number of builders assigned to a single task. |
| `--max-targets-per-task` | `5` | Maximum number of target units inside a single task. |
| `--units-file` | `plugins/faf-units/data/faf_units.json` | Path to the FAF units JSON file. When valid, builders/targets are sampled from real unit definitions. |

## Sampling pipeline

1. Load `faf-units` and split units into a **builder pool** (units with `build_power > 0`) and a **target pool** (units that have a build recipe).
2. Sample a realistic starting economy from the ACU plus 1–5 T1 engineers, 1–6 T1 power generators, and 0–4 T1 mass extractors, with storage filled to capacity.
3. For each task, sample builders from the builder pool and targets from the target pool.
4. Run the exact `faf-sim` simulator on the plan to get the ground-truth completion time.
5. Store the per-task feature sequence and label in SQLite.

The same steps are exposed as a fluent Rust pipeline for custom callers:

```rust
// crates/faf-build-prediction/src/data/generator.rs ~line 120 — DatasetGenerator::pipeline
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
    sequence_features TEXT NOT NULL, -- JSON array of per-task feature vectors
    target_time REAL NOT NULL,       -- simulated completion time, up to 10x the threshold
    is_practical INTEGER NOT NULL    -- 1 if completed within time_limit_seconds, else 0
);
```

Rows are inserted in batches and the file can be opened with any SQLite client for inspection.
