# 3. Hierarchical Policy Network

This chapter describes the learned hierarchical policy that guides MCTS. Instead of a single value network, the planner uses three cooperating networks:

1. **Macro network** — selects a concrete plan-graph edge.
2. **Build-power network** — decides how much build power to allocate to the selected edge.
3. **Engineer-squad network** — decides the `[T1, T2, T3]` engineer composition assigned to the edge.

The three networks are grouped into a single `PolicyBundle` so they can be saved, loaded, and optimized jointly.

## Why a three-network hierarchy?

The original design used a single macro-direction network that chose among abstract goals like `BuildPower`, `MoreMass`, or `MorePower`. A hand-written resolver then turned those directions into concrete build actions. That approach had two problems:

- The resolver duplicated logic that already existed in the plan graph and the simulator.
- The network never learned to make fine-grained decisions such as how many engineers to assign.

The new design lets the network reason at the level of real, executable plan-graph edges while still separating macro decisions from low-level allocation decisions.

## Network architecture

### Macro network

The macro network takes the current state features plus a three-dimensional shortfall feedback vector and outputs logits over every edge in the plan graph.

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 24 — MacroNet
#[derive(Module, Debug)]
pub struct MacroNet<B: Backend> {
    pub linear1: Linear<B>,
    pub linear2: Linear<B>,
    pub linear3: Linear<B>,
    pub activation: Relu,
}
```

Input size: `STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT` = `13 + 3 = 16`.  
Output size: number of edges in the `PlanGraph`.

The shortfall feedback tells the network how many idle engineers of each tech level were requested but unavailable in the previous tick. This lets the policy learn to wait or build more engineers when the current workforce is insufficient.

### Build-power network

The build-power network takes the base state features plus a one-hot encoding of the selected edge and outputs a scalar target build power.

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 82 — BuildPowerNet
#[derive(Module, Debug)]
pub struct BuildPowerNet<B: Backend> {
    pub linear1: Linear<B>,
    pub linear2: Linear<B>,
    pub linear3: Linear<B>,
    pub activation: Relu,
}
```

Input size: `STATE_FEATURE_COUNT + num_edges`.  
Output size: `1`.

At inference time the scalar is rounded to the nearest integer and clamped to the available idle build power.

### Engineer-squad network

The engineer-squad network takes the base state features plus the target build power and outputs a three-dimensional vector representing desired T1, T2, and T3 engineer counts.

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 138 — EngineerSquadNet
#[derive(Module, Debug)]
pub struct EngineerSquadNet<B: Backend> {
    pub linear1: Linear<B>,
    pub linear2: Linear<B>,
    pub linear3: Linear<B>,
    pub activation: Relu,
}
```

Input size: `STATE_FEATURE_COUNT + 1`.  
Output size: `3`.

At inference time the counts are rounded, clamped to available idle engineers, and then mapped to actual builder nodes by `select_squad_for_edge`.

## Policy bundle

The three networks are stored together in `PolicyBundle`:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 194 — PolicyBundle
#[derive(Module, Debug)]
pub struct PolicyBundle<B: Backend> {
    pub macro_net: MacroNet<B>,
    pub power_net: BuildPowerNet<B>,
    pub squad_net: EngineerSquadNet<B>,
}

impl<B: Backend> PolicyBundle<B> {
    pub fn new(device: &B::Device, num_edges: usize) -> Self;
}
```

`PolicyBundle::new` constructs all three networks and moves them to the requested device. The bundle is a `burn::module::Module`, so it can be recorded, loaded, and cloned like any other Burn module.

## State features

All three networks share a common base feature vector of size 13:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 10 — STATE_FEATURE_COUNT
pub const STATE_FEATURE_COUNT: usize = 13;

// crates/faf-sim/src/planner/mcts/features.rs ~line 14 — SHORTFALL_FEATURE_COUNT
pub const SHORTFALL_FEATURE_COUNT: usize = 3;
```

The 13 features are listed in `state_features`:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 29 — state feature order
// 0. net mass income   (scaled by 100)
// 1. net energy income (scaled by 1000)
// 2. mass storage ratio
// 3. energy storage ratio
// 4. total active build power (scaled by 100)
// 5. simulation time (scaled by 3600 s)
// 6. active mex fraction of cap
// 7. active pgen fraction of cap
// 8. active energy storage fraction of cap
// 9. active project count (scaled by 10)
// 10. has T2 factory
// 11. has T3 factory
// 12. has T3 engineer
```

They are intentionally economy-centric. Build orders in FAF are driven mainly by income, build power, and tech tier, so the network gets those directly instead of a huge one-hot unit roster.

## Inference

At inference time the planner performs three deterministic steps:

1. Compute `state_features_with_shortfall(state, units, config, shortfall)`.
2. Run the macro network, mask out illegal edges, and take `argmax`.
3. Run the build-power network and engineer-squad network for the selected edge.
4. Round, clamp, and resolve the squad into concrete `NodeId`s.

The core inference function is `macro_policy_plan` in `mcts::policy`:

```rust
// crates/faf-sim/src/planner/mcts/policy.rs ~line 52 — macro_policy_plan
fn macro_policy_plan(
    units: &Units,
    mut state: GraphState,
    goal_id: &UnitKind,
    policy_bundle: Option<PolicyBundle<TrainBackend>>,
    deterministic: bool,
    shortfall: &mut [f32; 3],
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    // ... feature computation, forward passes, action execution ...
}
```

If no bundle is provided, the function falls back to a deterministic greedy edge selector based on plan-graph structure. This is useful for testing without a trained model.

## Shortfall feedback loop

When the desired squad exceeds the available idle engineers, the unmet demand is stored as shortfall and fed back into the macro network on the next tick:

```rust
// crates/faf-sim/src/planner/mcts/policy.rs ~line 180 — shortfall update
*shortfall = shortfall_from_counts(&desired, &idle);
```

This lets the policy learn behaviors like "build more T1 engineers" or "wait for the current engineer squad to finish" without adding extra reward shaping.

## Action resolution

After the networks produce an edge, target power, and squad, the planner converts them into an executable `SimAction`. The conversion depends on the edge kind:

- **Build edge** → `SimAction::Build { unit_id, builders }`.
- **Upgrade edge** → `SimAction::Upgrade { target_unit_id, old_node, builders }`.
- **No-op / wait** → `SimAction::Wait`.

The helper `select_squad_for_edge` in `mcts::selections` maps the desired `[T1, T2, T3]` counts to actual idle engineer nodes:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 215 — select_squad_for_edge
pub fn select_squad_for_edge(
    edge: &PlanEdge,
    desired: [usize; 3],
    state: &GraphState,
    units: &Units,
) -> Vec<NodeId> {
    // prefer idle engineers closest to the edge source
}
```

The squad is clamped so it never exceeds the number of idle engineers of each tech level.

## Relationship to MCTS

Today the planner uses this hierarchical policy as a one-step decision maker. When full UCT search is implemented, each MCTS node will use the same three-network bundle to produce a prior distribution over edges and a default rollout policy. The macro network will guide selection, while the power and squad networks will resolve the selected edge into a concrete action during expansion.
