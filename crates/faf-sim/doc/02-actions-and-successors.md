# 2. Actions and Successors

The planner expands a state by choosing from the legal **plan-graph edges** derived from the `PlanGraph`. This chapter describes the two-level action model: high-level edges used by the learned policy, and low-level simulator commands that actually mutate `GraphState`.

## Two-level action model

The planner makes decisions at the level of **plan-graph edges**:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 151 — PlanEdge
#[derive(Debug, Clone)]
pub struct PlanEdge {
    /// Source node of the edge (builder for Build, unit being upgraded for Upgrade).
    pub source: UnitKind,
    /// Target node of the edge.
    pub target: UnitKind,
    /// Edge kind.
    pub kind: PlanEdgeKind,
}
```

An edge is still an abstract choice: it says *what* to build or upgrade, but not *which* idle engineers will execute it. Before the edge can be applied, the policy runs two more networks to decide the build power and the `[T1, T2, T3]` engineer squad, and then the squad is resolved into concrete `NodeId`s.

The abstraction is useful because the macro network reasons over a small, stable, plan-graph-constrained set of edges, while the simulator still consumes the existing concrete `SimAction` commands.

## Stable edge indexing

The macro network outputs one logit per edge, so the edge list must have a stable order. `PlanEdgeIndex` builds that list once from a `PlanGraph`:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 162 — PlanEdgeIndex
#[derive(Debug, Clone)]
pub struct PlanEdgeIndex {
    edges: Vec<PlanEdge>,
}

impl PlanEdgeIndex {
    pub fn new(plan: &PlanGraph) -> Self;
    pub fn len(&self) -> usize;
    pub fn get(&self, idx: usize) -> Option<&PlanEdge>;
    pub fn legal_mask(&self, state: &GraphState, units: &Units, config: &PlannerConfig) -> Vec<bool>;
    pub fn to_selection_option(&self, idx: usize, state: &GraphState, units: &Units, config: &PlannerConfig) -> Option<SelectionOption>;
}
```

`legal_mask` returns a boolean vector the same length as the edge list. A position is `true` only if:

- the edge's source unit is completed and active in `state`,
- the edge's target is not already completed or under construction,
- for build edges, there is an idle builder capable of building the target and the target does not exceed a storage cap,
- for upgrade edges, there is a finished source unit and an idle builder capable of performing the upgrade.

The mask is what makes the macro network's `argmax` safe: illegal edges are forced to a very negative logit before the softmax or argmax.

## From edges to selection options

A legal edge index can be converted back to a `SelectionOption` for execution:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 27 — SelectionOption
pub enum SelectionOption {
    Build(UnitKind),
    Upgrade { from: UnitKind, to: UnitKind },
    Assist(NodeId),
}
```

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 219 — PlanEdgeIndex::to_selection_option
pub fn to_selection_option(
    &self,
    idx: usize,
    state: &GraphState,
    units: &Units,
    config: &PlannerConfig,
) -> Option<SelectionOption> {
    let edge = self.edges.get(idx)?;
    if !is_edge_legal(edge, state, units, config) {
        return None;
    }
    match edge.kind {
        PlanEdgeKind::Build => Some(SelectionOption::Build(edge.target.clone())),
        PlanEdgeKind::Upgrade => Some(SelectionOption::Upgrade {
            from: edge.source.clone(),
            to: edge.target.clone(),
        }),
    }
}
```

`Assist` is part of the `SelectionOption` vocabulary but is not generated from plan-graph edges; it is reserved for future expansions that assign idle engineers to already-started projects.

## Resolving the engineer squad

After the macro network selects an edge, the build-power network outputs a scalar target build power and the engineer-squad network outputs desired `[T1, T2, T3]` counts. Those counts are clamped to the actual idle engineers of each tech level and then mapped to real builder nodes by `select_squad_for_edge`:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 418 — select_squad_for_edge
pub fn select_squad_for_edge(
    edge: &PlanEdge,
    desired: [usize; ENGINEER_TECH_LEVELS],
    state: &GraphState,
    units: &Units,
) -> Vec<NodeId> {
    let predicate: Box<dyn Fn(NodeId) -> bool> = match edge.kind {
        PlanEdgeKind::Build => {
            Box::new(|id: NodeId| units.can_build(&state.graph[id].unit_id, &edge.target))
        }
        PlanEdgeKind::Upgrade => { /* ... */ }
    };

    let buckets = idle_engineers_by_tech(state, units, predicate);
    let mut squad = Vec::new();
    for (i, bucket) in buckets.iter().enumerate() {
        let take = desired[i].min(bucket.len());
        squad.extend_from_slice(&bucket[..take]);
    }
    squad
}
```

The function prefers the highest build-rate engineers within each tech level, so a T2 engineer with a higher build rate is chosen before a slower one.

If the desired squad exceeds the available idle engineers, the difference is recorded as **shortfall** and fed back into the macro network on the next tick. This feedback loop lets the policy learn to build or upgrade engineers before retrying an edge that previously starved.

## Low-level `SimAction`

The concrete simulator commands are the existing `SimAction` enum, now extended to carry multiple builders:

```rust
// crates/faf-sim/src/planner/search.rs ~line 21 — SimAction enum
pub enum SimAction {
    Build {
        unit_id: UnitKind,
        builders: Vec<NodeId>,
    },
    Upgrade {
        target_unit_id: UnitKind,
        old_node: NodeId,
        builders: Vec<NodeId>,
    },
    Assist {
        project_node: NodeId,
        builders: Vec<NodeId>,
    },
    Wait,
}
```

- `Build` starts a new project for a unit, assigning one or more idle builders to it.
- `Upgrade` starts a new project for the higher-tier unit, retires the source slot, and assigns a squad of builders.
- `Assist` adds all specified idle engineers to an already-started project.
- `Wait` advances the simulator by one tick.

`Build` and `Upgrade` accept a `Vec<NodeId>` so that a squad of engineers can be committed immediately. The simulator assigns the total build power of all listed builders to the project.

## Execution in the one-step policy

The current one-step planner ties the pieces together in `macro_policy_plan`:

```rust
// crates/faf-sim/src/planner/mcts/policy.rs ~line 54 — macro_policy_plan (abbreviated)
fn macro_policy_plan(...) -> Result<PlanResult, PlannerError> {
    // 1. forward through macro network, masked argmax over legal edges
    // 2. forward through build-power network for the selected edge
    // 3. forward through engineer-squad network
    // 4. resolve squad, build SimAction::Build/Upgrade, execute
    // 5. update shortfall feedback
}
```

If no legal edge exists, or the resolved squad is empty, the planner issues `SimAction::Wait` for one tick and records the shortfall.

## Storage caps and adjacency

`EnergyStorage` remains a first-class `Build` target. The simulator tracks how many energy storages are adjacent to each pgen and applies the FAF adjacency bonus: `+12.5%` energy production per storage, up to four storages (`+50%`).

Mass storage is no longer an independent build target. Instead, capping a T2 or T3 mex with four mass storages is modeled as an upgrade:

- `Mex(T2) -> CapT2Mex`
- `Mex(T3) -> CapT3Mex`
- `CapT2Mex -> CapT3Mex`

The `CapT2Mex` and `CapT3Mex` unit definitions include the +50% mass adjacency bonus directly in their mass income, so no per-mex adjacency map is needed for mass storage.

## Why the branching factor matters

The edge list is already much smaller than the raw successor list would be because the `PlanGraph` prunes away units that are not on the path to the goal. Even so, a state with several idle engineers and factories may have many legal edges. The policy bundle keeps the decision cheap:

1. Compute the legal mask once with `PlanEdgeIndex::legal_mask`.
2. Run the three networks in three small forward passes.
3. Resolve the selected edge into a concrete `SimAction`.

This is `O(num_edges)` forward work per decision, not a tree expansion.

## Legal move validation

Not every edge is valid in every state. The simulator rejects actions that violate constraints:

- A busy builder cannot start a new project.
- A builder cannot build or upgrade a unit it is not capable of building.
- A builder cannot upgrade a unit that has no registered upgrade target.
- A builder cannot assist a non-existent project.

The edge legality check pre-filters most illegal actions, and `PlanEdgeIndex::to_selection_option` plus the simulator's `start_project`/`start_upgrade_project` methods catch the rest. The planner handles rejection by issuing `Wait` and trying again on the next tick.

## Key design choice: discrete actions, continuous time

Actions are discrete (build this unit, upgrade that unit, assist, wait), but time is continuous. The simulator advances by a fixed `dt` each tick. The planner therefore operates on a discretized decision grid. The choice of `dt` matters:

- A small `dt` gives finer control but issues more decisions.
- A large `dt` is faster but may miss tight timing windows.

The default `PlannerConfig::dt` is `1.0` second.
