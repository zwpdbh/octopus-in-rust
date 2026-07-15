# `train` command

Train a small LSTM sequence model to predict build-plan completion time from an initial economy snapshot and a plan.

## Usage

```bash
faf-sim-cli train --dataset data/build_prediction_dataset.db --output-dir data/build_prediction_artifacts
```

## Example with tuned parameters

For a larger dataset or when you want a more expressive model, increase the epochs and hidden size:

```bash
cargo run --release -p faf-sim-cli -- train \
  --dataset data/build_prediction_dataset.db \
  --output-dir data/build_prediction_artifacts \
  --epochs 100 \
  --batch-size 64 \
  --hidden-size 256 \
  --learning-rate 0.001 \
  --dropout 0.2 \
  --weight-decay 1e-5 \
  --time-weight-power 0.5
```

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--dataset` | required | Path to the SQLite dataset produced by `dataset generate`. |
| `--output-dir` | `data/build_prediction_artifacts` | Directory where model artifacts are saved. |
| `--epochs` | `10` | Number of training epochs. |
| `--batch-size` | `64` | Training batch size. |
| `--learning-rate` | `0.001` | Adam learning rate. |
| `--hidden-size` | `128` | LSTM hidden size. |
| `--dropout` | `0.0` | Dropout probability on the LSTM output. |
| `--weight-decay` | `0.0` | L2 weight decay for Adam. |
| `--time-weight-power` | `0.0` | Loss weighting power. Positive values up-weight fast plans so the model does not ignore the rare practical region. |

## Model architecture

The predictor is a single-layer LSTM that processes the build queue task-by-task. Each task is encoded as a 27-dimensional vector containing:

- the initial economy snapshot (production, storage, caps)
- builder aggregates (count, build power, maintenance)
- target aggregates (costs, build time, production, maintenance, storage)
- cumulative economy contributions from all earlier tasks in the plan

The cumulative deltas give the model a direct signal that, for example, a mass extractor built in Task 0 increases the mass income available when Task 1 starts.

The final LSTM hidden state is projected to a single `log(completion_time)` value. Exponentiating gives the predicted wall-clock time.

## Time-weighted loss

Randomly sampled plans are usually slow, so the dataset often contains far more "not practical" samples than "practical" ones. Standard MSE therefore optimizes mostly for the slow region and can overpredict fast-plan times.

`--time-weight-power` solves this by weighting each sample with `raw_time^{-power}`:

- `0.0` — unweighted MSE (default).
- `0.5` — moderate up-weighting of fast plans. A good starting point.
- `1.0` — strong up-weighting of fast plans.

The loss is still MSE on `log(completion_time)`; only the per-sample contribution is scaled.

## Output artifacts

The command writes three files to `--output-dir`:

- `config.json`: model architecture and training hyperparameters.
- `model`: trained Burn model weights.
- `norm.json`: per-feature min/max normalization params used during training.

These artifacts are consumed by the [`predict`](04-predict.md) command.
