# `dataset generate` command

Generate a SQLite dataset of simulated build plans and their completion times for training the predictor.

## Usage

```bash
faf-sim-cli dataset generate --samples 10000 --output dataset.sqlite
```

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--samples` | `10000` | Number of random `(eco, plan)` samples to generate. |
| `--output` | `data/build_prediction_dataset.db` | Path to the output SQLite file. |
| `--time-limit-seconds` | `600` | Simulation cutoff; slower plans are labeled as not practical. |
| `--max-tasks` | `5` | Maximum number of tasks in a generated plan. |
| `--max-builders-per-task` | `3` | Maximum number of builders assigned to a single task. |
| `--max-targets-per-task` | `5` | Maximum number of target units inside a single task. |

## Output schema

The SQLite file contains a single table `samples`:

```sql
CREATE TABLE samples (
    id INTEGER PRIMARY KEY,
    initial_eco TEXT NOT NULL,  -- JSON of EcoSnapshot
    plan TEXT NOT NULL,           -- JSON array of BuildTask
    features TEXT NOT NULL,       -- JSON array of scalar features
    time_seconds REAL NOT NULL,   -- simulated completion time, or time_limit_seconds
    is_practical INTEGER NOT NULL -- 1 if completed within time_limit_seconds, else 0
);
```

Rows are inserted in batches and the file can be opened with any SQLite client for inspection.
