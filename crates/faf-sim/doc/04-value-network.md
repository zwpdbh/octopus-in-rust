# 4. Building the Policy Network in Burn

This chapter is the heart of the tutorial: a single `burn::module::Module` that implements the direction-only policy. We will see how Burn's typed tensors and `Module` derive make it possible to express a small MLP policy in ordinary Rust, and why that small network is enough for this problem.

## Design overview

Instead of outputting one giant logit vector over all possible units, the policy uses a single **direction head** that picks a strategic focus:

1. `IncreaseMass`
2. `IncreaseEnergy`
3. `IncreaseBP`
4. `IncreaseEnergyStorage`
5. `Goal`
6. `UpgradeTech`

A separate deterministic heuristic layer converts the chosen direction into a concrete `SimAction`. The direction head shares a small backbone with the state featurizer so economy features are processed only once per decision.

> **Historical note:** Earlier versions of this network had additional heads for factory upgrades, concrete edges, target build power, and engineer squads. Those heads have been removed; the current architecture relies on the heuristic layer for those decisions.

## The `HierarchicalPolicyNet` struct

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 40 — HierarchicalPolicyNet
#[derive(Module, Debug)]
pub struct HierarchicalPolicyNet<B: Backend> {
    backbone1: Linear<B>,
    backbone2: Linear<B>,
    activation: Relu,
    direction_head: Linear<B>,
}
```

`#[derive(Module)]` is the only macro magic. It makes the struct recordable, loadable, movable to a device, and compatible with Burn's optimizers. The generic `B: Backend` means the same definition works for inference (`NdArray`) and training (`Autodiff<NdArray>`).

## Construction

The constructor sizes the backbone and direction head from the feature count, hidden sizes, and number of directions.

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 57 — HierarchicalPolicyNet::new
pub fn new(device: &B::Device) -> Self {
    let backbone_input = STATE_FEATURE_COUNT;
    let backbone_hidden = 128;
    let latent_dim = 64;

    Self {
        backbone1: LinearConfig::new(backbone_input, backbone_hidden).init(device),
        backbone2: LinearConfig::new(backbone_hidden, latent_dim).init(device),
        activation: Relu::new(),
        direction_head: LinearConfig::new(latent_dim, DIRECTION_COUNT).init(device),
    }
}
```

Burn layer configs (`LinearConfig::new(in, out).init(device)`) create the weight and bias tensors on the requested device. The input dimensions encode exactly what each head sees:

- `direction_head`: 64-D latent → 6 directions.

## Shared backbone

The backbone turns the 11-D input vector into a 64-D latent vector:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 79 — latent backbone
pub(crate) fn latent(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
    let x = self.backbone1.forward(features);
    let x = self.activation.forward(x);
    let x = self.backbone2.forward(x);
    self.activation.forward(x)
}
```

This is a plain two-layer MLP. The direction head consumes the latent vector produced by the backbone.

## Direction head

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 92 — direction head
pub(crate) fn direction_logits(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
    self.direction_head.forward(latent)
}
```

Output shape: `[batch, DIRECTION_COUNT]` where `DIRECTION_COUNT = 6`.

## Convenience evaluators

During MCTS and training we often evaluate a single state at a time. Burn's batched operations work fine with batch size `1`, so the crate provides small helpers that take a `Vec<f32>` and return Rust primitives:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 97 — evaluate_direction
pub fn evaluate_direction(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
    let tensor = tensor_from_vec(&features, device);
    let logits = self.direction_logits(self.latent(tensor));
    logits.into_data().as_slice::<f32>().unwrap().to_vec()
}
```

This helper is not required by Burn; it is just an ergonomic wrapper around `Tensor::from_data`, `into_data`, and `as_slice`.

## Inference

At inference time the planner performs three deterministic steps:

1. Compute `state_features(state, units, config)`.
2. Run the direction head, mask out illegal directions, and take `argmax`.
3. Run the heuristic layer to convert the selected direction into a concrete `SimAction` and execute it.

The core inference function is `macro_policy_plan` in `mcts::policy`:

```rust
// crates/faf-sim/src/planner/mcts/policy.rs ~line 44 — macro_policy_plan
pub(crate) fn macro_policy_plan(
    units: &Units,
    mut state: SimulationState,
    goal: &Goal,
    policy_bundle: Option<&dyn ValueNet>,
    deterministic: bool,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    // ... feature computation, forward pass, heuristic execution ...
}
```

If no bundle is provided, the function falls back to a freshly-initialized MLP value net. This is useful for testing without a trained model, although the resulting actions are essentially random.

## Relationship to MCTS and training

The same `HierarchicalPolicyNet` is used in two places:

1. **Training rollouts** — sample a direction from the masked softmax. No tree is built; the policy is sampled directly.
2. **MCTS priors and rollouts** — convert direction softmax probabilities into prior probabilities for legal directions, and play out the greedy policy from a leaf to estimate its value.

Because the network is a single `Module`, training, MCTS priors, and MCTS rollouts all share one set of weights and one serialization format.

Next we look at the reward signal that tells the policy whether its decisions are good.
