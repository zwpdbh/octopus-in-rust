# faf-sim-cli

Research CLI for training and running two independent FAF planners:

- **eco** — grows mass income as fast as possible.
- **rush** — learns when an economy is strong enough to start rushing a target unit.

## Commands

```sh
# Train the eco network (no target unit needed).
cargo run --release --bin faf-sim -- train eco -e 5000 -m 10000 \
  --dt 1.0 --grad-clip 1.0 \
  --target-mass-income 1000 \
  --max-mex-count 12

# Train the rush network for a target unit.
cargo run --release --bin faf-sim -- train rush -e 5000 -m 10000 \
  --dt 1.0 --grad-clip 1.0 \
  -t 25m --max-mex-count 12 \
  uef novaxcenter

# Simulate the eco planner from an initial ACU state.
cargo run --bin faf-sim -- simulate eco -n 200 --dt 1.0

# Simulate the rush policy for a target unit (requires a trained model).
cargo run --release --bin faf-sim -- simulate rush -s policy:mlp:greedy uef novaxcenter
```

### CPU-only backend

When CUDA is unavailable, disable the default features and enable the CPU backend:

```sh
cargo run --no-default-features --features cpu --release --bin faf-sim -- \
  train rush -e 5000 -m 10000 --grad-clip 1.0 uef novaxcenter
```

## What is being trained

### `train eco`

Trains a standalone economy-expansion network with five outputs covering the eco directions:
`IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`, `IncreaseEnergyStorage`, and `UpgradeTech`.
The reward is based on mass-income growth and energy/mass stall penalties.
The episode ends when the target mass income is reached or the step cap is exhausted.

### `train rush`

Trains a hierarchical policy network with:

- an **eco head** that chooses one of the five eco directions, and
- a **rush head** that decides whether the economy is ready to start the goal.

Training uses REINFORCE on the eco head and a supervised rush target derived from short rollouts.

## Training parameters reference

### Common flags

| Flag                | Default      | Description                                                                                              |
| ------------------- | ------------ | -------------------------------------------------------------------------------------------------------- |
| `-e, --episodes`    | **required** | Total number of training episodes.                                                                       |
| `-m, --max-steps`   | `500`        | Maximum simulator steps per episode.                                                                     |
| `--dt`              | `1.0`        | Fixed simulator timestep in seconds.                                                                     |
| `--grad-clip`       | none         | Global L2 gradient-clipping threshold. `1.0` is a good default.                                          |
| `--max-mex-count`   | `12`         | Maximum number of mass extractors (including capped upgrades) active at once.                            |
| `--energy-stall-penalty` | `20.0`  | Penalty applied each step when energy storage is empty.                                                  |
| `--mass-stall-penalty`   | `1.0`   | Penalty applied each step when mass storage is empty.                                                    |
| `-r, --resume`      | off          | Resume training from the existing model, if one exists.                                                  |
| `--fresh`           | off          | Delete any existing checkpoint before training.                                                          |

### Eco-only flags

| Flag                       | Default  | Description                                                              |
| -------------------------- | -------- | ------------------------------------------------------------------------ |
| `--target-mass-income`     | `1000.0` | Mass income per second that ends an episode.                             |
| `--reward-mass-income-coef`| `0.1`    | Coefficient for the mass-income delta reward.                            |
| `--epsilon-start`          | `0.3`    | Initial epsilon for exploration.                                         |
| `--epsilon-end`            | `0.01`   | Final epsilon after decay.                                               |
| `--epsilon-decay-episodes` | `1000`   | Number of episodes over which epsilon decays.                            |

### Rush-only flags

| Flag                          | Default  | Description                                                                                     |
| ----------------------------- | -------- | ----------------------------------------------------------------------------------------------- |
| `-t, --target-time`           | none     | Stop early once the best completion time is at most this duration (e.g. `-t 30m`).             |
| `--reward-bp-coef`            | `0.05`   | Coefficient for the build-power delta reward.                                                   |
| `--reward-mass-income-coef`   | `0.1`    | Coefficient for the mass-income delta reward.                                                   |
| `--reward-energy-income-coef` | `0.0`    | Coefficient for the energy-income delta reward.                                                 |
| `--eco-rollout-horizon-secs`  | `60.0`   | Horizon for the phantom-goal eco rollout.                                                       |
| `--rush-rollout-cap-secs`     | `300.0`  | Maximum seconds for the real-goal rush rollout.                                                 |
| `--rollout-bp-fraction`       | `0.8`    | Fraction of total build power assigned to phantom/rush goal projects.                           |
| `--mass-reward-coef`          | `0.01`   | Coefficient scaling mass spent during the eco rollout.                                          |
| `--goal-finish-base-reward`   | `100.0`  | Base reward for finishing the real goal within the rush cap.                                    |
| `--goal-too-early-penalty`    | `-10.0`  | Penalty for picking Goal when the goal cannot finish within the rush cap.                       |
| `--epsilon-start`             | `0.3`    | Initial epsilon for Goal-only exploration.                                                      |
| `--rush-threshold`            | `0.5`    | Rush probability threshold above which Goal is chosen.                                          |
| `--quiet`                     | off      | Suppress per-episode and progress output.                                                       |
| `--text`                      | off      | Print plain-text progress instead of opening the live dashboard.                                |

## Recommended starting points

### Eco training

```sh
cargo run --release --bin faf-sim -- \
  train eco -e 5000 -m 10000 \
  --dt 1.0 --grad-clip 1.0 \
  --target-mass-income 1000 \
  --max-mex-count 12
```

### Rush training

```sh
cargo run --release --bin faf-sim -- \
  train rush -e 5000 -m 10000 -t 25m \
  --dt 1.0 --grad-clip 1.0 \
  --max-mex-count 12 \
  uef novaxcenter
```

`-m 10000` gives the agent enough horizon to finish a T4 target (~160 minutes of game time at `--dt 1.0`). Lower it only after you have confirmed the policy can reach the goal.

## Simulating

### `simulate eco`

Runs the eco planner for a fixed number of steps, using either the heuristic or a trained eco model:

```sh
cargo run --release --bin faf-sim -- simulate eco -n 200 --dt 1.0

# With a trained eco model.
cargo run --release --bin faf-sim -- simulate eco -n 200 --dt 1.0 -m data/models/mlp-eco
```

### `simulate rush`

Loads the trained rush model for the target and runs a full build-order simulation:

```sh
cargo run --release --bin faf-sim -- simulate rush -s policy:mlp:greedy uef novaxcenter
```

## Model files

| Command        | Model path                                      |
| -------------- | ----------------------------------------------- |
| `train eco`    | `data/models/mlp-eco`                           |
| `train rush`   | `data/models/mlp-<faction>-<unit>`              |

Use `--fresh` to delete an existing checkpoint and start from scratch.

## Important: give the episode enough horizon

`-m` is a **cap**, not a fixed length. An episode stops immediately when the goal finishes. If the cap is too short, every episode will time out before reaching the goal and the policy will miss the large goal-completion reward.

For T4 rush targets, `-m 10000` (≈160 minutes of game time at `--dt 1.0`) is a safe starting point. For eco training the cap only needs to be large enough to reach the target mass income.
