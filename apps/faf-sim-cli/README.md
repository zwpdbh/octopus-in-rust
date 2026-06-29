## Commands

```sh
# Generate an SVG plan graph for a target unit.
cargo run --bin faf-sim -- plan uef novaxcenter
cargo run --release --bin faf-sim -- plan uef novaxcenter

# Train the hierarchical policy for a target unit.
# -e  : number of training episodes
# -m  : maximum simulator steps per episode (cap; episode ends early if the goal is reached)
# -r  : resume from an existing model file (optional)
# --patience <N> : stop early if no new best time for N episodes after the first success
# --quiet        : suppress per-episode and progress output
# -t <duration>  : stop early once the best time is at most this (e.g. -t 30m)
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 uef novaxcenter
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 --patience 1000 uef novaxcenter
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 --patience 1000 -r uef novaxcenter

# Simulate a trained policy. The default strategy is greedy argmax over the learned policy.
cargo run --bin faf-sim -- simulate uef novaxcenter
cargo run --release --bin faf-sim -- simulate uef novaxcenter
# Use an explicit strategy. `:greedy` (or `:deterministic`) makes the simulation reproducible.
cargo run --release --bin faf-sim -- simulate -s mcts:100:mlp:greedy uef novaxcenter
```

## What is being trained

The `train` command learns a **hierarchical policy bundle** of three small networks:

1. **Macro network** — selects a concrete plan-graph edge.
2. **Build-power network** — decides how much build power to allocate to that edge.
3. **Engineer-squad network** — decides the `[T1, T2, T3]` engineer composition.

Training uses REINFORCE with epsilon-greedy exploration, periodic greedy evaluation, and supervised fine-tuning on the best discovered trajectory. Full UCT tree search is not implemented yet; `simulate` currently runs the trained bundle as a one-step greedy policy.

## Important: give the episode enough horizon

`-m` is a **cap**, not a fixed length. An episode stops immediately when the goal unit finishes. If the cap is too short, every episode will time out before reaching the goal, the trainer will report `0/N reached`, and the policy will miss the large goal-completion reward.

For UEF Novax Center, `-m 5000` (≈83 minutes of game time) is enough for a good policy; the saved model from a successful run completes in about **35 minutes**. If you use `-m 2000`, most episodes will time out before the goal can finish.

## Plateau-based early stopping

Long runs can waste time once the policy stops improving. Use `--patience` to stop automatically:

```sh
cargo run --release --bin faf-sim -- train -e 10000 -m 5000 --patience 1000 uef novaxcenter
```

This counts episodes **after the first successful episode**. If no new best completion time is found for 1000 episodes, training stops and the best-seen model is saved.

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

## Important: model format changed

The policy is now a three-network hierarchical bundle. Models saved before this redesign have a different architecture and will fail to load with a dimension-mismatch error. Delete old checkpoints or retrain:

```sh
rm data/models/mlp-*.mpk
```

No `.trajectory.json` files are produced or used anymore.
