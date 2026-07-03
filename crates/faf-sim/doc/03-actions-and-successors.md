# 3. Actions and the Plan Graph

The raw action space of a build-order game is huge: at every tick you could build any unit you are capable of building, upgrade any existing structure, or assign any idle engineer to any active project. This chapter explains how `faf-sim` reduces that space to a small, structured set of **plan-graph edges** that the policy network can reason over.

## The universal plan graph

`faf-sim` builds one **universal plan graph** that contains all common units up to T3 (commander, factories, engineers, mexes, pgens, storages) plus the prerequisite factory and engineer upgrades. A goal-specific view is created by attaching a single synthetic `Goal` node to the T3 engineer. The same fixed graph shape is used for every target of the same tech level; only the synthetic goal's cost and build time change. The `faf-sim plan` command renders this graph with a placeholder **Target** node so the T3-engineer-only goal edge is visible.

```rust
// crates/faf-sim/src/planner/plan_graph.rs ~line 42 — EdgeCategory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeCategory {
    /// Edges that increase mass income.
    IncreaseMass,
    /// Edges that increase energy income or storage.
    IncreaseEnergy,
    /// Edges that increase build power.
    IncreaseBP,
    /// The single edge that builds the abstract goal.
    Goal,
}
```

Every edge in the plan graph is tagged with one of these four categories. The category tells the network what strategic focus the edge serves:

- `IncreaseMass` — edges that increase mass income (extractors and mass-storage caps).
- `IncreaseEnergy` — edges that increase energy income or storage (power generators and energy storage).
- `IncreaseBP` — edges that increase total build rate (commander, factories, factory upgrades, engineers, and engineer upgrades).
- `Goal` — the single synthetic edge that builds the abstract goal.

The network first picks a direction, then scores only the edges inside that direction. This factorization keeps the action head small and interpretable.

## Plan edges

A `PlanEdge` is a stable, typed action candidate. Each edge carries two orthogonal labels:

- `kind` — an [`EdgeAction`] that says *how* the action is executed.
- `category` — an [`EdgeCategory`] that says *what strategic focus* the edge serves.

```rust
// crates/faf-sim/src/planner/plan_graph.rs ~line 23 — EdgeAction
/// Concrete action an edge represents in the plan graph.
///
/// This describes *how* the action is executed. It is orthogonal to
/// `EdgeCategory`, which describes the strategic focus of the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeAction {
    /// Source unit constructs the target unit or goal.
    Build,
    /// Source unit is upgraded into the target unit.
    Upgrade,
}
```

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 197 — PlanEdge
#[derive(Debug, Clone)]
pub struct PlanEdge {
    /// Source node (builder for builds, unit being upgraded for upgrades).
    pub source: PlanNode,
    /// Target node (unit to build/upgrade into, or the abstract goal).
    pub target: PlanNode,
    /// Edge action: build a new unit/goal, or upgrade an existing unit.
    pub kind: EdgeAction,
    /// Strategic focus of this edge.
    category: EdgeCategory,
}

impl PlanEdge {
    /// Strategic focus of this edge.
    pub fn category(&self) -> EdgeCategory { self.category }
    /// Source unit kind, if the source is a concrete unit.
    pub fn source_unit(&self) -> Option<&UnitKind> { self.source.as_unit() }
    /// Target unit kind, if the target is a concrete unit.
    pub fn target_unit(&self) -> Option<&UnitKind> { self.target.as_unit() }
    /// Target goal, if the target is the abstract goal.
    pub fn target_goal(&self) -> Option<&Goal> { self.target.as_goal() }
}
```

- `source` is the builder required for a build edge, or the unit being upgraded for an upgrade edge.
- `target` is either a concrete unit or the abstract `Goal` node.
- `kind` is `Build` (construct target) or `Upgrade` (transform source into target).
- `category` is the strategic focus. `EdgeCategory::categorize` treats a `Goal` target as `EdgeCategory::Goal`.

For example, upgrading `Factory(T1) → Factory(T2)` has `kind = Upgrade` and `category = IncreaseBP`, while building `Factory(T1) → Engineer(T1)` has `kind = Build` and `category = IncreaseBP`.

The macro network outputs one logit per edge, so the edge list must have a stable order. `PlanEdgeIndex` builds that list once from a `PlanGraph`:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 232 — PlanEdgeIndex
#[derive(Debug, Clone)]
pub struct PlanEdgeIndex {
    edges: Vec<PlanEdge>,
}

impl PlanEdgeIndex {
    pub fn new(plan: &PlanGraph) -> Self;
    pub fn len(&self) -> usize;
    pub fn get(&self, idx: usize) -> Option<&PlanEdge>;
    pub fn legal_mask(&self, state: &SimulationState, units: &Units, config: &PlannerConfig) -> Vec<bool>;
    pub fn category_mask(&self, category: EdgeCategory) -> Vec<bool>;
    pub fn legal_mask_for_category(&self, state, units, config, category) -> Vec<bool>;
    pub fn legal_category_mask(&self, state, units, config) -> Vec<bool>;
}
```

`legal_mask` returns a boolean vector the same length as the edge list. A position is `true` only if:

- the edge's source unit is completed and active in `state`,
- the edge's target is not already completed or under construction,
- for build edges, there is an idle builder capable of building the target and the target does not exceed a storage cap,
- for upgrade edges, there is a finished source unit and an idle builder capable of performing the upgrade.

The masks are what make the network's `argmax` safe: illegal directions and edges are forced to a very negative logit before the softmax or argmax.

## From edges to selection options

A legal edge index can be converted back to a `SelectionOption` for execution:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 28 — SelectionOption
pub enum SelectionOption {
    /// Build a new unit of the given kind.
    Build(UnitKind),
    /// Upgrade an existing `from` unit into `to`.
    Upgrade { from: UnitKind, to: UnitKind },
    /// Build the abstract goal directly (once the required builder is owned).
    BuildGoal(Goal),
    /// Assist an active project.
    Assist(NodeId),
}
```

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 335 — PlanEdgeIndex::to_selection_option
pub fn to_selection_option(
    &self,
    idx: usize,
    state: &SimulationState,
    units: &Units,
    config: &PlannerConfig,
) -> Option<SelectionOption> {
    let edge = self.edges.get(idx)?;
    if !is_edge_legal(edge, state, units, config) {
        return None;
    }
    match edge.kind {
        EdgeAction::Build => match edge.target_goal() {
            Some(goal) => Some(SelectionOption::BuildGoal(*goal)),
            None => Some(SelectionOption::Build(edge.target_unit()?.clone())),
        },
        EdgeAction::Upgrade => Some(SelectionOption::Upgrade {
            from: edge.source_unit()?.clone(),
            to: edge.target_unit()?.clone(),
        }),
    }
}
```

`Assist` is part of the `SelectionOption` vocabulary and is generated for every active project when an idle engineer is available. `BuildGoal` is generated from the synthetic goal edge once the required builder (currently a T3 engineer) is owned and no goal project is already active. The current MCTS rollout and policy use build, upgrade, and goal edges.

## Resolving the engineer squad

After the macro network selects an edge, the build-power head outputs a scalar target build power and the engineer-squad head outputs desired `[T1, T2, T3]` counts. Those counts are clamped to the actual idle engineers of each tech level and then mapped to real builder nodes by `select_squad_for_edge`:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 565 — select_squad_for_edge
pub fn select_squad_for_edge(
    edge: &PlanEdge,
    desired: [usize; ENGINEER_TECH_LEVELS],
    state: &SimulationState,
    units: &Units,
) -> Vec<NodeId> {
    let predicate: Box<dyn Fn(NodeId) -> bool> = match edge.kind {
        EdgeAction::Build => {
            if let Some(goal) = edge.target_goal() {
                Box::new(move |id: NodeId| can_build_goal(&state.graph[id].unit_id, goal))
            } else {
                let target = edge.target_unit().expect("build target must be unit or goal").clone();
                Box::new(move |id: NodeId| units.can_build(&state.graph[id].unit_id, &target))
            }
        }
        EdgeAction::Upgrade => { /* ... */ }
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

If no idle engineers can satisfy the target, the heuristic emits `SimAction::Wait` for one tick.

## Low-level `SimAction`

The concrete simulator commands are the existing `SimAction` enum, extended to carry multiple builders:

```rust
// crates/faf-sim/src/planner/action.rs ~line 17 — SimAction enum
pub enum SimAction {
    /// Build a unit with the given builders.
    Build {
        unit_id: UnitKind,
        builders: Vec<NodeId>,
    },
    /// Upgrade an existing unit in-place to a higher-tier blueprint.
    Upgrade {
        target_unit_id: UnitKind,
        old_node: NodeId,
        builders: Vec<NodeId>,
    },
    /// Build the abstract goal with the given builders.
    BuildGoal { goal: Goal, builders: Vec<NodeId> },
    /// Assist an active project with additional builders.
    Assist {
        project_node: NodeId,
        builders: Vec<NodeId>,
    },
    /// Advance time without issuing a command.
    Wait,
}
```

- `Build` starts a new project for a unit, assigning one or more idle builders to it.
- `Upgrade` starts a new project for the higher-tier unit, retires the source slot, and assigns a squad of builders.
- `Assist` adds all specified idle engineers to an already-started project.
- `Wait` advances the simulator by one tick.

`Build` and `Upgrade` accept a `Vec<NodeId>` so that a squad of engineers can be committed immediately. The simulator assigns the total build power of all listed builders to the project.

## Execution in the one-step policy

The one-step planner ties the pieces together in `macro_policy_plan`:

```rust
// crates/faf-sim/src/planner/mcts/policy.rs ~line 44 — macro_policy_plan (abbreviated)
fn macro_policy_plan(...) -> Result<PlanResult, PlannerError> {
    // 1. forward through direction head and pick a legal direction
    // 2. heuristic converts direction to a concrete SimAction
    // 3. execute SimAction::Build/Upgrade/BuildGoal
}
```

If no legal direction exists, or the heuristic cannot assign builders, the planner issues `SimAction::Wait` for one tick.

## Storage caps and adjacency

`EnergyStorage` remains a first-class `Build` target. The simulator tracks how many energy storages are adjacent to each pgen and applies the FAF adjacency bonus: `+12.5%` energy production per storage, up to four storages (`+50%`).

Mass storage is no longer an independent build target. Instead, capping a T2 or T3 mex with four mass storages is modeled as an upgrade:

- `Mex(T2) -> CapT2Mex`
- `Mex(T3) -> CapT3Mex`
- `CapT2Mex -> CapT3Mex`

The `CapT2Mex` and `CapT3Mex` unit definitions include the +50% mass adjacency bonus directly in their mass income, so no per-mex adjacency map is needed for mass storage.

## Why the branching factor matters

The edge list is much smaller than the raw successor list would be because the universal `PlanGraph` contains only the common prerequisite chain and a single synthetic goal node. Even so, a state with several idle engineers and factories may have many legal edges. The hierarchical policy keeps the decision cheap:

1. Compute the legal category mask once with `PlanEdgeIndex::legal_category_mask`.
2. Run the direction head.
3. Compute the legal edge mask for the chosen category.
4. Run the action head.
5. Run the power and squad heads for the selected edge.
6. Resolve the selected edge into a concrete `SimAction`.

This is `O(num_edges)` forward work per decision, not a tree expansion. MCTS multiplies that work by its iteration budget, but the per-decision cost remains bounded.

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

Now that we know what actions look like, we can build the network that chooses them.
