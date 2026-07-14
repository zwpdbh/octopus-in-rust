# `train` command

Train a small neural network to predict build-plan completion time from an initial economy snapshot and a plan.

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
  --learning-rate 0.001
```

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--dataset` | required | Path to the SQLite dataset produced by `dataset generate`. |
| `--output-dir` | `data/build_prediction_artifacts` | Directory where model artifacts are saved. |
| `--epochs` | `10` | Number of training epochs. |
| `--batch-size` | `64` | Training batch size. |
| `--learning-rate` | `0.001` | Adam learning rate. |
| `--hidden-size` | `128` | Hidden layer size of the MLP. |

## Output artifacts

The command writes three files to `--output-dir`:

- `config.json`: model architecture and training hyperparameters.
- `model`: trained Burn model weights.
- `norm.json`: input feature normalization parameters (mean and std).

These artifacts are consumed by the [`predict`](04-predict.md) command.
