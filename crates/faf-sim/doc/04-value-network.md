# 4. Building the Policy Network in Burn

This chapter is the heart of the tutorial: a single `burn::module::Module` that implements the hierarchical policy. We will see how Burn's typed tensors and `Module` derive make it possible to express a four-head policy in ordinary Rust.

## Design overview

Instead of outputting one giant logit vector over all possible units, the policy factorizes the decision into four stages:

1. **Direction head** — pick a strategic focus: `Mass`, `Energy`, `BuildPower`, or `Progress`.
2. **Action head** — pick a concrete plan-graph edge inside that focus.
3. **Power head** — decide how much total build power to assign to that edge.
4. **Squad head** — decide the `[T1, T2, T3]` engineer counts that deliver that power.

The first two heads are discrete (categorical). The last two are continuous (Gaussian regression). All four heads share a common backbone so that economy features are processed only once per decision.

## The `HierarchicalPolicyNet` struct

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 48 — HierarchicalPolicyNet
#[derive(Module, Debug)]
pub struct HierarchicalPolicyNet<B: Backend> {
    backbone1: Linear<B>,
    backbone2: Linear<B>,
    activation: Relu,
    direction_head: Linear<B>,
    action_hidden: Linear<B>,
    action_head: Linear<B>,
    power_hidden: Linear<B>,
    power_head: Linear<B>,
    squad_hidden: Linear<B>,
    squad_head: Linear<B>,
}
```

`#[derive(Module)]` is the only macro magic. It makes the struct recordable, loadable, movable to a device, and compatible with Burn's optimizers. The generic `B: Backend` means the same definition works for inference (`NdArray`) and training (`Autodiff<NdArray>`).

## Construction

The constructor sizes every layer from the feature count, hidden sizes, number of directions, and number of plan-graph edges. Because the universal plan graph is fixed, `num_edges` is the same for every goal of the same tech level; a single trained bundle can be reused across many targets as long as the edge count matches.

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 63 — HierarchicalPolicyNet::new
pub fn new(device: &B::Device, num_edges: usize) -> Self {
    let backbone_input = STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT;
    let backbone_hidden = 128;
    let latent_dim = 64;

    Self {
        backbone1: LinearConfig::new(backbone_input, backbone_hidden).init(device),
        backbone2: LinearConfig::new(backbone_hidden, latent_dim).init(device),
        activation: Relu::new(),
        direction_head: LinearConfig::new(latent_dim, DIRECTION_COUNT).init(device),
        action_hidden: LinearConfig::new(latent_dim + DIRECTION_COUNT, 128).init(device),
        action_head: LinearConfig::new(128, num_edges).init(device),
        power_hidden: LinearConfig::new(latent_dim + num_edges, 64).init(device),
        power_head: LinearConfig::new(64, 1).init(device),
        squad_hidden: LinearConfig::new(latent_dim + 1, 64).init(device),
        squad_head: LinearConfig::new(64, 3).init(device),
    }
}
```

Burn layer configs (`LinearConfig::new(in, out).init(device)`) create the weight and bias tensors on the requested device. The input dimensions encode exactly what each head sees:

- `direction_head`: 64-D latent → 4 directions.
- `action_head`: 64-D latent + 4-D one-hot direction → hidden → `num_edges` logits.
- `power_head`: 64-D latent + `num_edges`-D one-hot edge → hidden → 1 scalar.
- `squad_head`: 64-D latent + 1-D power → hidden → 3 engineer counts.

## Shared backbone

The backbone turns the 16-D input vector into a 64-D latent vector:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 84 — latent backbone
pub(crate) fn latent(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
    let x = self.backbone1.forward(features);
    let x = self.activation.forward(x);
    let x = self.backbone2.forward(x);
    self.activation.forward(x)
}
```

This is a plain two-layer MLP. Because every head calls `latent` first, a single forward pass can reuse the latent vector for all four heads. In training we call `latent` once per step and then feed it into each head.

## The four heads

### Direction head

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 91 — direction head
pub(crate) fn direction_logits(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
    self.direction_head.forward(latent)
}
```

Output shape: `[batch, DIRECTION_COUNT]` where `DIRECTION_COUNT = 4`.

### Action head

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 97 — action head
pub(crate) fn action_logits(
    &self,
    latent: Tensor<B, 2>,
    direction_one_hot: Tensor<B, 2>,
) -> Tensor<B, 2> {
    let x = Tensor::cat(vec![latent, direction_one_hot], 1);
    let x = self.action_hidden.forward(x);
    let x = self.activation.forward(x);
    self.action_head.forward(x)
}
```

The action head is conditioned on the chosen direction via concatenation. This means the network can learn different edge preferences for `Mass` versus `Progress` directions.

### Power head

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 109 — power head
pub(crate) fn power_mean(
    &self,
    latent: Tensor<B, 2>,
    edge_one_hot: Tensor<B, 2>,
) -> Tensor<B, 2> {
    let x = Tensor::cat(vec![latent, edge_one_hot], 1);
    let x = self.power_hidden.forward(x);
    let x = self.activation.forward(x);
    self.power_head.forward(x)
}
```

Output shape: `[batch, 1]`. At inference the scalar is rounded to the nearest integer and clamped to the available idle build power.

### Squad head

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 121 — squad head
pub(crate) fn squad_means(&self, latent: Tensor<B, 2>, power: Tensor<B, 2>) -> Tensor<B, 2> {
    let x = Tensor::cat(vec![latent, power], 1);
    let x = self.squad_hidden.forward(x);
    let x = self.activation.forward(x);
    self.squad_head.forward(x)
}
```

Output shape: `[batch, 3]`, representing desired T1, T2, and T3 engineer counts. At inference the counts are rounded, clamped to available idle engineers, and mapped to actual builder nodes by `select_squad_for_edge`.

## Convenience evaluators

During MCTS and training we often evaluate a single state at a time. Burn's batched operations work fine with batch size `1`, so the crate provides small helpers that take a `Vec<f32>` and return Rust primitives:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 129 — evaluate_direction
pub fn evaluate_direction(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
    let tensor = tensor_from_vec(&features, device);
    let logits = self.direction_logits(self.latent(tensor));
    logits.into_data().as_slice::<f32>().unwrap().to_vec()
}
```

These helpers are not required by Burn; they are just ergonomic wrappers around `Tensor::from_data`, `into_data`, and `as_slice`.

## Inference

At inference time the planner performs four deterministic steps:

1. Compute `state_features_with_shortfall(state, units, config, shortfall)`.
2. Run the direction head, mask out illegal directions, and take `argmax`.
3. Run the action head for that direction, mask out illegal edges, and take `argmax`.
4. Run the power and squad heads for the selected edge, then round, clamp, and resolve the squad into concrete `NodeId`s.

The core inference function is `macro_policy_plan` in `mcts::policy`:

```rust
// crates/faf-sim/src/planner/mcts/policy.rs ~line 54 — macro_policy_plan
fn macro_policy_plan(
    units: &Units,
    mut state: GraphState,
    goal: &Goal,
    policy_bundle: Option<PolicyBundle<TrainBackend>>,
    deterministic: bool,
    shortfall: &mut [f32; 3],
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    // ... feature computation, forward passes, action execution ...
}
```

If no bundle is provided, the function falls back to a deterministic greedy edge selector based on plan-graph structure. This is useful for testing without a trained model.

## Relationship to MCTS

The same `HierarchicalPolicyNet` is used in three places:

1. **Training rollouts** — sample directions, edges, power, and squad with added Gaussian noise.
2. **MCTS priors** — convert direction/action softmax probabilities into a prior probability for each legal edge.
3. **MCTS rollouts** — play out the greedy policy from a leaf to estimate its value.

Because the network is a single `Module`, all three usages share one set of weights and one serialization format.

## Shortfall feedback loop

When the desired squad exceeds the available idle engineers, the unmet demand is stored as shortfall and fed back into the macro network on the next tick:

```rust
// crates/faf-sim/src/planner/mcts/policy.rs ~line 156 — shortfall update
*shortfall = shortfall_from_counts(&desired, &idle);
```

This lets the policy learn behaviors like "build more T1 engineers" or "wait for the current engineer squad to finish" without adding extra reward shaping.

Next we look at the reward signal that tells the policy whether its decisions are good.
