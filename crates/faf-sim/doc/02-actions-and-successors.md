# 2. Actions and Successors

The MLP planner expands a state by choosing from the legal **selection options** derived from the `PlanGraph`. This chapter describes the two-level action model: high-level options used by the learned policy, and low-level simulator commands that actually mutate `GraphState`.

## Two-level action model

The planner makes decisions at the level of **selection options**:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 19 — SelectionOption enum
pub enum SelectionOption {
    /// Build a new unit of the given kind.
    Build(UnitKind),
    /// Upgrade an existing `from` unit into `to`.
    Upgrade { from: UnitKind, to: UnitKind },
    /// Assist an active project. Builders are resolved at execution time.
    Assist(NodeId),
}
```

A selection option is an abstract choice. Before it can be executed it is converted into a concrete `SimAction`:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 170 — SelectionOption::to_sim_action
impl SelectionOption {
    pub(crate) fn to_sim_action(&self, state: &GraphState, units: &Units) -> Option<SimAction> {
        // ...
    }
}
```

The separation is useful because the MLP policy reasons over a small, plan-graph-constrained set of options, while the simulator still consumes the existing `SimAction` commands.

## Generating options from the plan graph

`SelectionPools` is a wrapper around the legal `SelectionOption`s for the current state. It derives them by walking the static `PlanGraph`:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 36 — SelectionPools
pub struct SelectionPools {
    options: Vec<SelectionOption>,
}
```

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 42 — SelectionPools::new
pub fn new(plan: &PlanGraph, state: &GraphState, units: &Units) -> Self {
    // ... walks plan-graph edges, emits Build/Upgrade options,
    //     then adds Assist options for active projects with idle engineers ...
}
```

For each edge `source -> target` in the plan graph:

- The source must be a completed, active unit in the current state.
- The target must not already be completed or under construction.
- For a **build** edge, the source must be an idle builder capable of building the target.
- For an **upgrade** edge, there must be a finished source unit and an idle builder capable of performing the upgrade.

`Assist` options mention only the active project node. The engineers that will assist it are chosen when the option is converted into a `SimAction`. The wrapper exposes the final list through `options`:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 106 — SelectionPools::options
pub fn options(&self) -> &[SelectionOption] {
    &self.options
}
```

## From options to executable actions

The policy scores each option returned by `SelectionPools::options`, but not every scored option can be executed immediately. The planner filters by `SelectionOption::to_sim_action` before sampling. For example, a `Build` option needs a specific idle builder node; if the only capable builder became busy during the current tick, the option is skipped and the planner issues `Wait`.

## Low-level `SimAction`

The concrete simulator commands are still the existing `SimAction` enum:

```rust
// crates/faf-sim/src/planner/search.rs ~line 21 — SimAction enum
pub enum SimAction {
    Build {
        unit_id: UnitKind,
        builder: NodeId,
    },
    Upgrade {
        target_unit_id: UnitKind,
        old_node: NodeId,
        builder: NodeId,
    },
    Assist {
        project_node: NodeId,
        builders: Vec<NodeId>,
    },
    Wait,
}
```

- `Build` starts a new project for a unit, assigning one idle builder to it.
- `Upgrade` starts a new project for the higher-tier unit and retires the source slot.
- `Assist` adds all idle engineers to an already-started project.
- `Wait` advances the simulator by one tick.

The current MLP planner usually assigns a single builder; future successors may assign multiple builders to a project.

## Why the branching factor matters

The option list is already much smaller than the raw successor list would be because the `PlanGraph` prunes away units that are not on the path to the goal. Even so, a state with several idle engineers and factories may have many legal options. The policy network keeps the decision cheap:

1. Generate options once with `SelectionPools::new` and read them with `SelectionPools::options`.
2. Score all options in a single batched forward pass.
3. Sample one option and convert it to a `SimAction`.

This is `O(n_candidates)` forward work per decision, not a tree expansion.

## Legal move validation

Not every `SelectionOption` is valid in every state. The simulator rejects actions that violate constraints:

- A busy builder cannot start a new project.
- A builder cannot build or upgrade a unit it is not capable of building.
- A builder cannot upgrade a unit that has no registered upgrade target.
- A builder cannot assist a non-existent project.

The candidate generator pre-filters most illegal actions, and `SelectionOption::to_sim_action` plus the simulator's `start_project`/`assist_project` methods catch the rest. The planner handles rejection by issuing `Wait` and trying again on the next tick.

## Key design choice: discrete actions, continuous time

Actions are discrete (build this unit, upgrade that unit, assist, wait), but time is continuous. The simulator advances by a fixed `dt` each tick. The planner therefore operates on a discretized decision grid. The choice of `dt` matters:

- A small `dt` gives finer control but issues more decisions.
- A large `dt` is faster but may miss tight timing windows.

The default `PlannerConfig::dt` is `10.0` seconds.
