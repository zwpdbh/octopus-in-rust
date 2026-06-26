# 3. The Value Network

A value network estimates how good a state is. In our case, it predicts the remaining time until the goal unit finishes. This chapter explains why we need it, how to convert `GraphState` into network inputs, and how to train the network in Rust with `burn`.

## Why not random rollouts?

Classic MCTS evaluates a leaf by playing random moves to the end of the episode and averaging the outcome. For FAF this is wasteful:

- The horizon is long; a single rollout may take thousands of ticks.
- Random build orders are almost always terrible, so the average is noisy.
- The reward is sparse: you learn nothing until the goal finishes.

A value network replaces the rollout. It looks at the state once and predicts the outcome. This makes MCTS fast enough to run at every decision tick.

## Featurizing the state

The value network cannot consume a `GraphState` directly. We need a fixed-size vector. The current scaffold defines a feature count:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 14 — FEATURE_COUNT
pub const FEATURE_COUNT: usize = 64;
```

The `featurize` function is currently a placeholder:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 19 — featurize (placeholder)
pub fn featurize(_state: &GraphState, _goal_id: &str, _units: &Units) -> Vec<f32> {
    todo!("state featurization is not yet implemented")
}
```

A good first feature set is hand-crafted. It should include:

- Current simulation time.
- Economy snapshot: mass/energy income, storage ratios, stall flags.
- Builder summary: idle count, busy count, factories by tech level.
- Goal distance: tech level of the goal, missing prerequisites, owned prerequisites.
- Aggregate counts of key unit categories.

Normalization matters. The network trains more reliably when inputs are roughly zero-mean and unit-variance. Scale time by a fixed horizon, income by a typical late-game value, and counts by a plausible maximum.

## Network architecture

The current value network is a small multi-layer perceptron:

```rust
// crates/faf-sim/src/planner/mcts/value_net.rs ~line 27 — ValueNet
#[derive(Module, Debug, Clone)]
pub struct ValueNet {
    linear1: Linear<Backend>,
    activation: Relu,
    linear2: Linear<Backend>,
    output: Linear<Backend>,
}
```

It maps `feature_count -> 128 -> 64 -> 1`. The output is a scalar value estimate. A good target is the normalized negative remaining time:

```text
value(state) = -remaining_seconds / TIME_SCALE
```

With `TIME_SCALE = 600.0` (ten minutes), most values fall in `[-1, 0]`. A value of `-0.5` means the network expects the goal to finish in about five minutes from this state.

The `Backend` is fixed to `NdArray` in the scaffold:

```rust
// crates/faf-sim/src/planner/mcts/value_net.rs ~line 17 — Backend type alias
pub type Backend = NdArray;
```

This keeps the public API simple. Later you can swap in `WGPU` or `CUDA` by changing this alias.

## Training data

The easiest way to train the value net is supervised learning on rollout data:

1. Run the beam planner or a random policy on many goals.
2. For every visited `GraphState`, record the true final completion time.
3. Pair each state with its target value: `-completion_time / TIME_SCALE`.
4. Train with mean-squared error loss.

Because the simulator is deterministic, the target is exact. There is no noisy environment to model. The main challenge is coverage: the network must see states that are similar to those MCTS will ask about.

A single training batch therefore contains rows of the form:

```text
[features] -> target_value
```

## Training loop sketch

Using `burn`, the loop looks roughly like this:

```rust
// docref: example
use burn::optim::AdamConfig;
use burn::tensor::{Tensor, TensorData};

let optimizer = AdamConfig::new().init();
let mut value_net = ValueNet::new(FEATURE_COUNT, &device);

for batch in training_data.batches(BATCH_SIZE) {
    let inputs = Tensor::from_data(
        TensorData::new(batch.features_flat(), [BATCH_SIZE, FEATURE_COUNT]),
        &device,
    );
    let targets = Tensor::from_data(
        TensorData::new(batch.targets(), [BATCH_SIZE, 1]),
        &device,
    );

    let predictions = value_net.forward(inputs);
    let loss = predictions.sub(targets).powf_scalar(2.0).mean();

    let grads = loss.backward();
    value_net = optimizer.step(loss, value_net, grads);
}
```

Keep a held-out validation set. If training loss drops but validation loss rises, the network is memorizing the training distribution and will fail on unseen MCTS states.

## Loading the network into the planner

After training, save the checkpoint with `burn`'s serialization and load it inside `faf-sim`. The `ValueNet` struct is the same at training and inference time, so no conversion is needed.

```rust
// docref: example
let value_net: ValueNet<Backend> = load_checkpoint("value_net.ckpt", &device)?;
let features = featurize(state, goal);
let value = value_net.evaluate_single(features, &device);
```

A positive value means the state is predicted to finish faster than the scale; a negative value means slower. MCTS uses this scalar to rank leaves.

## When the network is uncertain

Early in training, the value net will be wrong. Two mitigations help:

1. **Fallback heuristic.** If the network's confidence is low (or training data is sparse for this region), fall back to a hand-written heuristic.
2. **Mixed evaluation.** Blend the network value with a shallow rollout to completion. This is more expensive but more robust while the net is learning.

The long-term goal is to drop the fallback once the network generalizes well across the goal suite.
