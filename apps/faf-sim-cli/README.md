## Strategy

- Use MCTS + value network (RL).
- The RL uses the eco state to generate a probability over candidates discovered from the plan graph.

## Commands

```sh
# Generate an SVG of the universal plan graph (includes a placeholder Target node
# that can only be built by a T3 engineer).
cargo run --bin faf-sim -- plan
cargo run --release --bin faf-sim -- plan

# Train the hierarchical policy for a target unit.
cargo run --release --bin faf-sim -- train -e 5000 -m 10000 \
  --epsilon 0.3 --epsilon-final 0.01 --epsilon-decay-episodes 5000 \
  --dt 1.0 --grad-clip 1.0 \
  -t 25m --max-mex-count 10 \
  --resume \
  uef novaxcenter

# Simulate a trained policy. The default strategy is greedy argmax over the learned policy.
cargo run --bin faf-sim -- simulate uef novaxcenter
cargo run --release --bin faf-sim -- simulate uef novaxcenter

# Use an explicit strategy. `:greedy` (or `:deterministic`) makes the simulation reproducible.
cargo run --release --bin faf-sim -- simulate -s mcts:100:mlp:greedy uef novaxcenter
```

## What is being trained

The `train` command learns a single **hierarchical policy network** with a shared backbone and four heads:

1. **Direction head** — picks a strategic focus (`Mass`, `Energy`, `BuildPower`, `Progress`).
2. **Action head** — selects a concrete plan-graph edge inside that focus.
3. **Power head** — decides how much build power to allocate to that edge.
4. **Squad head** — decides the `[T1, T2, T3]` engineer composition.

Training uses REINFORCE with epsilon-greedy exploration, periodic greedy evaluation, and supervised fine-tuning on the best discovered trajectory. `simulate` uses the trained network as both the MCTS prior and the leaf rollout policy.

## Training parameters reference

| Flag                       | Default      | Description                                                                                                                                                       |
| -------------------------- | ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `-e, --episodes`           | **required** | Total number of training episodes. Use `0` to run until `-t` is satisfied or the process is interrupted.                                                          |
| `-m, --max-steps`          | `500`        | Maximum simulator steps per episode. This is a cap: the episode stops earlier if the goal is reached. See the horizon advice below.                               |
| `--dt`                     | `1.0`        | Fixed simulator timestep in seconds. Smaller values make the simulation finer but need more steps to cover the same game time. `1.0` is a good default.           |
| `-t, --target-time`        | none         | Stop early once the best completion time is at most this duration (e.g. `-t 30m`, `-t 1h`, `-t 1800s`).                                                           |
| `--epsilon`                | `0.1`        | Initial epsilon-greedy exploration probability. Higher values mean more random actions early on.                                                                  |
| `--epsilon-final`          | `0.01`       | Final epsilon value after decay.                                                                                                                                  |
| `--epsilon-decay-episodes` | same as `-e` | Number of episodes over which epsilon linearly decays from `--epsilon` to `--epsilon-final`.                                                                      |
| `--no-epsilon-decay`       | off          | Keep epsilon constant at `--epsilon` for the whole run. Useful when resuming and you want to keep exploring.                                                      |
| `--grad-clip`              | none         | Global L2 gradient-clipping threshold. `1.0` is a good default for preventing REINFORCE divergence. Omit to disable clipping.                                     |
| `--max-mex-count`          | `12`         | Maximum number of mass extractors (including capped upgrades) that may be active at the same time. New mex builds are blocked at this cap; upgrades do not count. |
| `-r, --resume`             | off          | Resume training from the existing model for this target, if one exists.                                                                                           |
| `--fresh`                  | off          | Delete any existing checkpoint for this target before training.                                                                                                   |
| `--quiet`                  | off          | Suppress per-episode and progress output.                                                                                                                         |
| `--text`                   | off          | Print plain-text progress to stderr instead of opening the live dashboard.                                                                                        |

## Recommended starting points

### UEF Novax Center

A good first run that balances exploration and training time:

```sh
cargo run --release --bin faf-sim -- \
  train -e 5000 -m 10000 -t 25m \
  --epsilon 0.3 --epsilon-final 0.01 --epsilon-decay-episodes 5000 \
  --dt 1.0 \
  --grad-clip 1.0 \
  --max-mex-count 12 \
  uef novaxcenter
```

- `-m 10000` gives the agent enough horizon to finish a T4 target (~160 minutes of game time).
- `--epsilon 0.3` encourages broad exploration early; it decays to `0.01` over 5000 episodes.
- `-t 25m` stops early if the policy finds a 25-minute build order.
- `--grad-clip 1.0` keeps REINFORCE gradients stable.

A shorter smoke-test run to verify the setup:

```sh
cargo run --release --bin faf-sim -- \
  train -e 100 -m 10000 \
  --epsilon 0.3 --epsilon-final 0.01 --epsilon-decay-episodes 100 \
  --dt 1.0 --grad-clip 1.0 \
  uef novaxcenter
```

With the current heuristic you should see goal reaches within the first few episodes. If the first 20–30 episodes report `reached=false` every time, check the horizon (`-m`) and the exploration rate (`--epsilon`).

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

| Metric               | What it tells you                                                                                                     |
| -------------------- | --------------------------------------------------------------------------------------------------------------------- |
| **Episode Loss**     | REINFORCE policy loss for the finished episode. Should trend downward as the policy improves.                         |
| **Fine-Tune Loss**   | Supervised loss when fine-tuning on the best discovered trajectory. Should also decrease.                             |
| **Episode Steps**    | Number of simulator steps taken before the episode ended. Lower usually means the agent reached the goal faster.      |
| **Completion Time (min)** | Completion time in minutes when the goal was reached. Lower is better. Reported as "-" if the episode timed out.      |
| **Goal Reach**       | Percentage of episodes that reached the goal. You want this to climb toward 100%.                                     |
| **Epsilon**          | Current exploration probability. Should decay smoothly from `--epsilon` to `--epsilon-final`.                         |
| **Best Time (min)**      | Best completion time in minutes observed so far across training and greedy evaluations. Monotonically non-increasing. |
| **Greedy Eval Time (min)** | Completion time in minutes of periodic greedy (no exploration) rollouts. Measures true policy quality.                |
| **Episodes/sec**     | Training throughput. Higher is faster, but does not indicate learning quality.                                        |

When training is healthy you should see `Goal Reach` rise, `Episode Loss` fall, and `Best Time (min)` drop within the first few hundred episodes.

## Important: give the episode enough horizon

`-m` is a **cap**, not a fixed length. An episode stops immediately when the goal finishes. If the cap is too short, every episode will time out before reaching the goal, the trainer will report `0/N reached`, and the policy will miss the large goal-completion reward.

For UEF Novax Center, `-m 10000` (≈160 minutes of game time at `--dt 1.0`) is enough for an untrained policy to stumble onto a solution; the saved model from a successful run completes in about **35–40 minutes**. If you use `-m 2000`, most episodes will time out before the goal can finish.

## Simulate without a trained model

If you run `simulate` before training, the planner falls back to a randomly initialized network and runs MCTS with random priors over an 8-hour horizon. This is expected to be very slow and is useful only as a smoke test. Train first for real results.

## Controlling exploration

Training uses epsilon-greedy exploration. By default epsilon decays from `--epsilon` (default `0.1`) to `--epsilon-final` (default `0.01`) over the full run. If you resume training and want to keep exploring aggressively, disable the decay; epsilon will then stay at the value of `--epsilon`:

```sh
# Constant 10% random actions for the whole resumed run
cargo run --release --bin faf-sim -- train -e 10000 -m 10000 --no-epsilon-decay -r uef novaxcenter

# Constant 30% random actions
# (--epsilon-final is ignored when decay is disabled)
cargo run --release --bin faf-sim -- train -e 10000 -m 10000 --epsilon 0.3 --no-epsilon-decay uef novaxcenter
```

You can also keep the default decay but make it slower by setting `--epsilon-decay-episodes` larger than `-e`.

## Example training output

```text
Training MLP for UEF Novax Center
ep=   1 steps=  42 eps=0.3000 reached=true time=      52m 15.0s best=      52m 15.0s loss=   -2.3456
...
ep= 9500 steps=  38 eps=0.0100 reached=true time=      35m 23.0s best=      35m 23.0s loss=    1.0438
  greedy eval at ep=10000: time=35m 23.0s best=35m 23.0s
Fine-tuned best model on trajectory: epochs=100 loss=1.0438
Training complete: 9259/10000 episodes reached the goal
Best completion time: 35m 23.0s
Saved best-seen model to data/models/mlp-uef-novax-center
```

## Example simulate output

`simulate` loads the trained model automatically and prints the completion time, the final economy, and the build timeline:

```text
Strategy: mcts:100:mlp:greedy
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
2. **Increase exploration**. If `--epsilon` is too low the policy can get stuck in a local optimum. Try `--epsilon 0.3`.
3. **Check the horizon in game time**. Game time is `steps * dt`. With `--dt 1.0` and `-m 10000` the horizon is ~160 minutes; with `--dt 0.5` the same `-m` covers ~80 minutes.
4. **Use `--fresh`** if you resumed from a bad checkpoint.

### Loss explodes or `NaN`

Add or tighten gradient clipping: `--grad-clip 1.0`. If it still diverges, try a smaller `--epsilon` or a shorter `--epsilon-decay-episodes` so the policy spends less time taking very random actions while the value estimates are still unstable.

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
