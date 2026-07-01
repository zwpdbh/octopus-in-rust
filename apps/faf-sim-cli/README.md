## Strategy

- Use MCTS + value network (RL)
- The RL use the eco state to generate a probability over candidates discovered from plan graph.

## Commands

```sh
# Generate an SVG of the universal plan graph (includes a placeholder Target node
# that can only be built by a T3 engineer).
cargo run --bin faf-sim -- plan
cargo run --release --bin faf-sim -- plan

# Train the hierarchical policy for a target unit.
# -e  : number of training episodes
# -m  : maximum simulator steps per episode (cap; episode ends early if the goal is reached)
# -r  : resume from an existing model file (optional)
# --patience <N> : stop early if no new best time for N episodes after the first success
# --quiet        : suppress per-episode and progress output
# -t <duration>  : stop early once the best time is at most this (e.g. -t 30m)
# --no-epsilon-decay : keep exploration probability constant (useful for resuming a search)
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 uef novaxcenter
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 --patience 1000 uef novaxcenter
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 --patience 6000 -r uef novaxcenter

cargo run --release --bin faf-sim -- train \
  -e 30000 -m 5000 \
  --epsilon 0.3 --epsilon-final 0.01 --epsilon-decay-episodes 5000 \
  --patience 10000 \
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

## Important: give the episode enough horizon

`-m` is a **cap**, not a fixed length. An episode stops immediately when the goal finishes. If the cap is too short, every episode will time out before reaching the goal, the trainer will report `0/N reached`, and the policy will miss the large goal-completion reward.

For UEF Novax Center, `-m 5000` (≈83 minutes of game time) is enough for a good policy; the saved model from a successful run completes in about **35 minutes**. If you use `-m 2000`, most episodes will time out before the goal can finish.

## Simulate without a trained model

If you run `simulate` before training, the planner falls back to a randomly initialized network and runs MCTS with random priors over an 8-hour horizon. This is expected to be very slow and is useful only as a smoke test. Train first for real results.

## Plateau-based early stopping

Long runs can waste time once the policy stops improving. Use `--patience` to stop automatically:

```sh
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 --patience 1000 uef novaxcenter
```

This counts episodes **after the first successful episode**. If no new best completion time is found for 1000 episodes, training stops and the best-seen model is saved.

## Controlling exploration

Training uses epsilon-greedy exploration. By default epsilon decays from `--epsilon` (default `0.1`) to `--epsilon-final` (default `0.01`) over the full run. If you resume training and want to keep exploring aggressively, disable the decay; epsilon will then stay at the value of `--epsilon`:

```sh
# Constant 10% random actions for the whole resumed run
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 --no-epsilon-decay -r uef novaxcenter

# Constant 30% random actions
# (--epsilon-final is ignored when decay is disabled)
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 --epsilon 0.3 --no-epsilon-decay uef novaxcenter
```

You can also keep the default decay but make it slower by setting `--epsilon-decay-episodes` larger than `-e`.

## Example training output

```text
Training MLP for UEF Novax Center
ep=   1 steps=  42 eps=0.1000 reached=true time=      52m 15.0s best=      52m 15.0s loss=   -2.3456
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

## Important: model format changed

The policy is now a single hierarchical network with a fixed action-head dimension for the universal plan graph. Models saved before the abstract-goal redesign have a stale architecture and will fail to load with a dimension-mismatch error. Delete old checkpoints or retrain:

```sh
rm data/models/mlp-*.mpk
```

No `.trajectory.json` files are produced or used anymore.
