# 1. Modeling the Environment

In RL, every decision is made from a **state**. For FAF build-order optimization, the state is the simulator's `GraphState`. This chapter explains the structure of that state and how we turn it into a fixed-size feature vector that a Burn network can consume.

For the full formal model — exact definitions, constraints, and assumptions — see [`model.md`](./model.md). This chapter focuses on the RL-relevant view.

## From ACU to goal

The simulator starts with a single completed unit: the ACU (Armored Command Unit). Every other unit is built by assigning builders from the existing graph. The state therefore records:

- every unit that exists,
- when each unit started and finished,
- which builders contributed to which unit,
- the current economy.

Active construction projects are not stored separately; they are the graph nodes currently in the `Constructing` or `Upgrading` state.

```rust
// crates/faf-sim/src/sim/state.rs ~line 298 — GraphState (abbreviated)
pub struct GraphState {
    pub time: f64,
    pub graph: BuildGraph,
    pub economy: EconomyState,
    pub events: Vec<BuildEvent>,
}
```

The RL planner does not need to invent this representation; it reuses the existing simulator state as its node payload. It also does not need to read raw FAF data; all unit knowledge comes through the `Units` repository.

## Nodes and edges

Each node in the build graph represents one built-unit slot. It stores at least:

- `unit_id`: the current blueprint identifier, e.g., `URL0001` (ACU), `URB0101` (T1 land factory), `URL0402` (Monkeylord).
- `state`: a lifecycle tag that says whether the slot is under construction or finished, and whether it was constructed from scratch or upgraded from another unit.

A directed edge `A -> B` means unit `A` contributed build power to create unit `B`. Multiple incoming edges mean multiple builders assisted.

The initial graph contains exactly one node: the ACU, finished at time 0.

## Upgrades add new nodes

An upgrade creates a **new** node for the upgraded unit rather than reusing the old slot. The source node moves to `Replaced { into: new_node }`, so it is no longer counted as an active unit for economy or builder calculations. The new node starts as `Upgrading { from_unit_id: old_id }` and completes as `Upgraded { from_unit_id: old_id }`.

This keeps the graph history explicit: the old unit remains visible as a retired node, while the active unit set contains only the new upgraded unit.

## Builder constraints that shape the tree

Builder behavior creates most of the structure that the planner must reason about:

- A builder works on **exactly one target at a time**. Its build power is indivisible.
- A builder can contribute to many units over its lifetime, but the construction intervals must not overlap.
- To **start** a project, at least one assigned builder must be able to build the target according to the tech graph in `Units`.
- To **upgrade** a unit, the source unit must have a registered upgrade target in `Units.upgrade_table`.
- Other builders may **assist** a project even if they cannot build the target themselves, provided they are real builders (commanders, engineers, factories).
- For every edge `A -> B`: `finish_time(A) <= start_time(B)`.

These rules determine which candidates and `SimAction` expansions are legal from a given state.

## Economy and stall

The state also contains the economy:

- mass income per second,
- energy income per second,
- mass storage and cap,
- energy storage and cap.

When a resource-producing unit finishes, its production is added to the economy state immediately at its `finish_time`.

If available mass or energy cannot sustain the assigned build power, the effective build rate is reduced proportionally to the most-constrained resource. Energy stall is especially punishing because it slows both construction and mass income. A good RL policy will learn to avoid it.

For the current model:

- Energy-dependent systems (shields, radar, stealth) are ignored.
- Reclaim is not modeled.

## Featurizing the state

Neural networks need fixed-size inputs. The state featurizer compresses the variable-size `GraphState` into a 13-dimensional vector:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 15 — feature constants
pub const STATE_FEATURE_COUNT: usize = 13;
pub const SHORTFALL_FEATURE_COUNT: usize = 3;
```

The 13 features are listed in `state_features`:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 27 — state feature order
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

The macro network also receives a three-dimensional **shortfall** vector, which records how many idle engineers of each tech level were requested but unavailable in the previous tick. This feedback lets the policy learn to build or upgrade engineers before retrying an action that previously starved.

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 123 — state_features_with_shortfall
pub fn state_features_with_shortfall(
    state: &GraphState,
    units: &Units,
    config: &PlannerConfig,
    shortfall: [f32; SHORTFALL_FEATURE_COUNT],
) -> Vec<f32> {
    let mut features = state_features(state, units, config);
    features.extend_from_slice(&shortfall);
    features
}
```

## What the policy sees

From the network's point of view, a state is a snapshot it can evaluate and expand:

```text
GraphState
├── time
├── graph of completed and under-construction units
├── economy
└── events
    │
    ▼
state_features()  →  [f32; 13]
    │
    ▼
state_features_with_shortfall()  →  [f32; 16]
    │
    ▼
HierarchicalPolicyNet
```

The search loop uses the legal successors of this snapshot to grow the tree (see [chapter 4](03-actions-and-successors.md)).

## Objectives

1. **Primary:** minimize the completion time of the goal unit.
2. **Secondary:** among plans with the same primary time, maximize mass income per mass invested in economy up to that completion time. This prevents the optimizer from rewarding extra economy built after the goal is already reached.

## Assumptions

- Unit knowledge is provided by `Units`; the tech graph and upgrade table are fixed for a given `Units` instance.
- Build power, production, and drain are known per blueprint.
- The economy evolves deterministically given a schedule.
- A builder's build power is indivisible across concurrent targets.
- There is exactly one goal unit.
