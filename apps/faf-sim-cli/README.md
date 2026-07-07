## Strategy

- Use a learned direction policy network (REINFORCE).
- The policy uses the economy state to generate a probability distribution over high-level directions discovered from the plan graph.

## Commands

```sh
# Generate an SVG of the universal plan graph (includes a placeholder Target node
# that can only be built by a T3 engineer).
cargo run --bin faf-sim -- plan
cargo run --release --bin faf-sim -- plan

# Train the hierarchical policy for a target unit.
cargo run --release --bin faf-sim -- train -e 5000 -m 10000 \
  --dt 1.0 --grad-clip 1.0 \
  -t 25m --max-mex-count 10 \
  --resume \
  uef novaxcenter

# Simulate a trained policy. The default strategy is greedy argmax over the learned policy.
cargo run --bin faf-sim -- simulate uef novaxcenter
cargo run --release --bin faf-sim -- simulate uef novaxcenter
cargo run --release --bin faf-sim -- simulate uef novaxcenter  --strategy policy:mlp:greedy
# Use an explicit strategy. `:greedy` (or `:deterministic`) makes the simulation reproducible.
cargo run --release --bin faf-sim -- simulate -s policy:mlp:greedy uef novaxcenter
```

## What is being trained

The `train` command learns a single **hierarchical policy network** with a shared backbone and four heads:

1. **Direction head** — picks a strategic focus (`Mass`, `Energy`, `BuildPower`, `Progress`).
2. **Action head** — selects a concrete plan-graph edge inside that focus.
3. **Power head** — decides how much build power to allocate to that edge.
4. **Squad head** — decides the `[T1, T2, T3]` engineer composition.

Training uses REINFORCE with greedy action selection. `simulate` runs the trained policy once per decision tick, masks illegal directions, and commits to the highest-probability legal direction.

## Training parameters reference

| Flag                          | Default      | Description                                                                                                                                                           |
| ----------------------------- | ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `-e, --episodes`              | **required** | Total number of training episodes. Use `0` to run until `-t` is satisfied or the process is interrupted.                                                              |
| `-m, --max-steps`             | `500`        | Maximum simulator steps per episode. This is a cap: the episode stops earlier if the goal is reached. See the horizon advice below.                                   |
| `--dt`                        | `1.0`        | Fixed simulator timestep in seconds. Smaller values make the simulation finer but need more steps to cover the same game time. `1.0` is a good default.               |
| `-t, --target-time`           | none         | Stop early once the best completion time is at most this duration (e.g. `-t 30m`, `-t 1h`, `-t 1800s`).                                                               |
| `--grad-clip`                 | none         | Global L2 gradient-clipping threshold. `1.0` is a good default for preventing REINFORCE divergence. Omit to disable clipping.                                         |
| `--max-mex-count`             | `12`         | Maximum number of mass extractors (including capped upgrades) that may be active at the same time. New mex builds are blocked at this cap; upgrades do not count.     |
| `--reward-bp-coef`            | `0.05`       | Coefficient for the build-power delta reward. Set to `0.0` to disable.                                                                                                |
| `--reward-mass-income-coef`   | `0.1`        | Coefficient for the mass-income delta reward. Set to `0.0` to disable.                                                                                                |
| `--reward-energy-income-coef` | `0.0`        | Coefficient for the energy-income delta reward. Default is `0.0` so the agent learns power management from the energy stall penalty instead of a direct income bonus. |
| `--energy-stall-penalty`      | `20.0`       | Penalty applied each step when energy storage is empty (energy stall).                                                                                                |
| `--mass-stall-penalty`        | `1.0`        | Penalty applied each step when mass storage is empty (mass stall).                                                                                                    |
| `-r, --resume`                | off          | Resume training from the existing model for this target, if one exists.                                                                                               |
| `--fresh`                     | off          | Delete any existing checkpoint for this target before training.                                                                                                       |
| `--quiet`                     | off          | Suppress per-episode and progress output.                                                                                                                             |
| `--text`                      | off          | Print plain-text progress to stderr instead of opening the live dashboard.                                                                                            |

## Recommended starting points

### UEF Novax Center

A good first run that balances exploration and training time:

```sh
cargo run --release --bin faf-sim -- \
  train -e 5000 -m 10000 -t 25m \
  --dt 1.0 \
  --grad-clip 1.0 \
  --max-mex-count 12 \
  uef novaxcenter
```

- `-m 10000` gives the agent enough horizon to finish a T4 target (~160 minutes of game time).
- `-t 25m` stops early if the policy finds a 25-minute build order.
- `--grad-clip 1.0` keeps REINFORCE gradients stable.

A shorter smoke-test run to verify the setup:

```sh
cargo run --release --bin faf-sim -- \
  train -e 100 -m 10000 \
  --dt 1.0 --grad-clip 1.0 \
  uef novaxcenter
```

With the current heuristic you should see goal reaches within the first few episodes. If the first 20–30 episodes report `reached=false` every time, check the horizon (`-m`) and the mass-income reward coefficient (`--reward-mass-income-coef`).

### Starting completely fresh

```sh
cargo run --release --bin faf-sim -- \
  train -e 5000 -m 10000 --fresh --grad-clip 1.0 uef novaxcenter
```

### CPU-only backend

Use this when the GPU is not available or when the tiny network is CPU-bound:

```sh
cargo run --no-default-features --features cpu --release --bin faf-sim -- \
  train -e 5000 -m 10000 --grad-clip 1.0 uef novaxcenter
```

## Reading the training dashboard

The live dashboard shows one plot per metric. You can switch metrics with `←`/`→` and plot types (recent history / full history) with `↑`/`↓`.

| Metric                    | What it tells you                                                                                                                                                         |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Episode Loss**          | REINFORCE policy loss for the finished episode. Should trend downward as the policy improves.                                                                             |
| **Episode Steps**         | Number of simulator steps taken before the episode ended. Lower usually means the agent reached the goal faster.                                                          |
| **Completion Time (min)** | Completion time in minutes when the goal was reached. Lower is better. Reported as "-" if the episode timed out.                                                          |
| **Goal Reach**            | Sliding-window success rate over the last 100 episodes, plotted as a percentage. Higher is better.                                                                        |
| **Best Time (min)**       | Best completion time in minutes observed so far from episodes that reached the goal. Reported as "N/A" before any episode reaches the goal. Monotonically non-increasing. |
| **Episodes/sec**          | Training throughput. Higher is faster, but does not indicate learning quality.                                                                                            |

When training is healthy you should see `Goal Reach` rise, `Episode Loss` fall, and `Best Time (min)` drop within the first few hundred episodes.

### Dashboard controls

| Key         | Action                                                |
| ----------- | ----------------------------------------------------- |
| `←` / `→`   | Switch metric tab                                     |
| `↑` / `↓`   | Switch plot between recent history and full history   |
| `q`         | Open quit options                                     |
| `s`         | Stop training gracefully at the next episode boundary |
| `k`         | Kill the training process immediately (panic)         |
| `c` / `Esc` | Cancel quit and resume training                       |

When training completes, the dashboard shows a **Training Complete** popup. Press any key to dismiss it and inspect the final metrics, then press `q` to exit. After the TUI closes, the CLI prints a text summary.

## Important: give the episode enough horizon

`-m` is a **cap**, not a fixed length. An episode stops immediately when the goal finishes. If the cap is too short, every episode will time out before reaching the goal, the trainer will report `0/N reached`, and the policy will miss the large goal-completion reward.

For UEF Novax Center, `-m 10000` (≈160 minutes of game time at `--dt 1.0`) is enough for an untrained policy to stumble onto a solution; the saved model from a successful run completes in about **35–40 minutes**. If you use `-m 2000`, most episodes will time out before the goal can finish.

## Simulate without a trained model

If you run `simulate` before training, the planner uses a randomly initialized network and picks random legal directions over an 8-hour horizon. This is expected to be very slow and is useful only as a smoke test. Train first for real results.

## Controlling exploration

Training currently uses greedy action selection. Exploration will be reintroduced later as a separate mechanism (e.g. temperature-based sampling or parameter-space noise). For now the only lever is the reward coefficient: a larger `--reward-mass-income-coef` makes mass-income changes more influential, which can help the policy escape local optima where mass growth stalls.

## Example training output

```text
Training MLP for UEF Novax Center
ep=   1 steps=  42 reached=true time=      52m 15.0s best=      52m 15.0s loss=   -2.3456
...
ep= 9500 steps=  38 reached=true time=      35m 23.0s best=      35m 23.0s loss=    1.0438
Training complete: 9259/10000 episodes reached the goal
Best completion time: 35m 23.0s
Saved model to data/models/mlp-uef-novax-center
```

## Example simulate output

`simulate` loads the trained model automatically and prints the completion time, the final economy, and the build timeline:

```text
Strategy: policy:mlp:greedy
Simulate target: UEF Novax Center (XEB2402)
Loading trained model from data/models/mlp-uef-novax-center.mpk

Goal completed at 33m 21.0s (33.4m)

Final economy:
  Mass income:  63.0 / s
  Energy income: 2912.0 / s
  Mass storage:  5280 / 5280
  Energy storage: 13900 / 13900

Timeline:
        Time  Unit
------------  ----
    0m 31.0s  Land Factory (Factory(T1))
    ...
   33m 20.5s  Novax Center (Unique(UnitId("XEB2402")))

Build-order diagram written to:
  /tmp/faf-sim-simulate-uef-novax-center.svg
  file:///tmp/faf-sim-simulate-uef-novax-center.svg
```

## Troubleshooting

### `0/N reached` after many episodes

1. **Increase `-m`**. The most common cause is a step cap that is too short for the target to finish. For T4 units start with `-m 10000`.
2. **Adjust `--reward-mass-income-coef`**. A larger coefficient can help the policy escape local optima where mass growth stalls.
3. **Check the horizon in game time**. Game time is `steps * dt`. With `--dt 1.0` and `-m 10000` the horizon is ~160 minutes; with `--dt 0.5` the same `-m` covers ~80 minutes.
4. **Use `--fresh`** if you resumed from a bad checkpoint.

### Loss explodes or `NaN`

Add or tighten gradient clipping: `--grad-clip 1.0`. If it still diverges, try lowering the learning rate or using a smaller `--reward-bp-coef` so the policy spends less time taking very large steps while the value estimates are still unstable.

### Training is very slow

- Use the release build: `--release`.
- Use CUDA if available (default feature set).
- Lower `-m` only **after** you have confirmed the policy can reach the goal.

## Important: model format changed

The policy is now a single hierarchical network with a fixed action-head dimension for the universal plan graph. Models saved before the abstract-goal redesign have a stale architecture and will fail to load with a dimension-mismatch error. Delete old checkpoints or retrain:

```sh
rm data/models/mlp-*.mpk
```

No `.trajectory.json` files are produced or used anymore.
