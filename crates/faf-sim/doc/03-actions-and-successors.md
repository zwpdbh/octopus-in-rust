# 3. Actions and the Plan Graph

The raw action space of a build-order game is huge: at every tick you could build any unit you are capable of building, upgrade any existing structure, or assign any idle engineer to any active project. This chapter explains how `faf-sim` reduces that space to **six high-level directions** and a deterministic heuristic that turns a direction into a concrete simulator command.

## The universal plan graph

`faf-sim` builds one **universal plan graph** that contains all common units up to T3 (commander, factories, engineers, mexes, pgens, storages) plus the prerequisite factory and engineer upgrades. A goal-specific view is created by attaching a single synthetic `Goal` node to the T3 engineer. The same fixed graph shape is used for every target of the same tech level; only the synthetic goal's cost and build time change. The `faf-sim plan` command renders this graph with a placeholder **Target** node so the T3-engineer-only goal edge is visible.

The edges in the plan graph carry two orthogonal labels:

- `EdgeAction` — *how* the action is executed.
- `EdgeCategory` — *what strategic focus* the edge serves.

```rust
// crates/faf-sim/src/planner/plan_graph.rs ~line 35 — EdgeAction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeAction {
    /// Source unit constructs the target unit or goal.
    Build,
    /// Source unit is upgraded into the target unit.
    Upgrade,
}
```

```rust
// crates/faf-sim/src/planner/plan_graph.rs ~line 52 — EdgeCategory
pub enum EdgeCategory {
    IncreaseMass,
    IncreaseEnergy,
    IncreaseBP,
    IncreaseEnergyStorage,
    Goal,
    UpgradeTech,
}
```

The six categories are the output space of the direction-only policy network. The network does not choose individual edges; it chooses a category. A separate heuristic layer then scans the plan graph for the best legal edge inside that category.

| Category | Typical concrete actions |
| --- | --- |
| `IncreaseMass` | Build a mex, cap a T2/T3 mex, upgrade a mex. |
| `IncreaseEnergy` | Build a power generator, upgrade a pgen. |
| `IncreaseBP` | Build an engineer or factory. |
| `IncreaseEnergyStorage` | Build an energy storage. |
| `Goal` | Start the abstract goal project with a T3 engineer. |
| `UpgradeTech` | Upgrade a factory to the next tech level. |

## Plan nodes

A `PlanNode` is either a concrete unit kind or the abstract goal:

```rust
// crates/faf-sim/src/planner/plan_graph.rs ~line 112 — PlanNode
pub enum PlanNode {
    Unit(UnitKind),
    Goal(Goal),
}
```

The plan graph is a directed acyclic graph where an edge `A -> B` means "unit `A` can build or upgrade into `B`." For example:

- `ACU --Build--> Factory(T1)`
- `Factory(T1) --Build--> Engineer(T1)`
- `Factory(T1) --Upgrade--> Factory(T2)`
- `Engineer(T3) --Build--> Goal`

## Legality: which edges can execute now?

Not every plan-graph edge is legal in every state. `is_plan_edge_legal` checks the current `SimulationState` before an edge can be used:

```rust
// crates/faf-sim/src/planner/plan_graph.rs ~line 342 — is_plan_edge_legal
pub fn is_plan_edge_legal(
    action: EdgeAction,
    source: &PlanNode,
    target: &PlanNode,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> bool {
    let Some(source_kind) = source.as_unit() else { return false; };
    if !state.has_completed_unit(source_kind) { return false; }

    match action {
        EdgeAction::Build => {
            let can_build = match target.as_goal() {
                Some(goal) => {
                    !state.goal_reached(goal)
                        && !state.goal_project_active()
                        && can_build_goal(source_kind, goal)
                }
                None => {
                    let target_kind = target.as_unit().expect("build target must be unit or goal");
                    !state.has_completed_unit(target_kind)
                        && !state.active_target_unit_ids().contains(target_kind)
                        && !would_exceed_mex_cap(state, config, target_kind)
                }
            };
            can_build && is_idle_builder(state, units, source_kind)
        }
        EdgeAction::Upgrade => {
            let source_kind = source.as_unit().expect("upgrade source must be a unit");
            let target_kind = target.as_unit().expect("upgrade target must be a unit");
            can_upgrade(state, units, source_kind, target_kind)
        }
    }
}
```

For a build edge to be legal:

- the source unit must be completed,
- the target must not already be completed or under construction,
- there must be an idle builder capable of building the target.

For an upgrade edge to be legal:

- the source unit must be active and not busy,
- there must be an idle builder capable of performing the upgrade.

## From direction to concrete action

The heuristic layer is the bridge between the network's high-level direction and the simulator's concrete commands. Its entry point is `direction_to_action`:

```rust
// crates/faf-sim/src/planner/policy/heuristic.rs ~line 25 — direction_to_action
pub fn direction_to_action(
    direction: EdgeCategory,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
    goal: &Goal,
    plan: &PlanGraph,
) -> SimAction {
    match direction {
        EdgeCategory::IncreaseMass => pick_mass_action(plan, state, units, config),
        EdgeCategory::IncreaseEnergy => pick_energy_action(plan, state, units, config),
        EdgeCategory::IncreaseBP => pick_bp_action(plan, state, units, config),
        EdgeCategory::IncreaseEnergyStorage => pick_storage_action(plan, state, units, config),
        EdgeCategory::Goal => pick_goal_action(state, units, config, goal),
        EdgeCategory::UpgradeTech => pick_upgrade_action(plan, state, units, config),
    }
}
```

Each `pick_*_action` helper scans the plan graph for legal edges in its category and applies a domain-specific rule:

- `IncreaseMass` — pick the mass action with the shortest payback time.
- `IncreaseEnergy` — pick the highest-tech legal power generator.
- `IncreaseBP` — build the highest-tier engineer available.
- `IncreaseEnergyStorage` — build energy storage if legal.
- `Goal` — start the goal project with T3 engineers if possible.
- `UpgradeTech` — upgrade the lowest-tier idle factory first.

If no legal concrete action exists for the chosen direction, the helper returns `SimAction::Wait`. This means the network can sample any direction; the environment handles infeasible choices safely.

## Low-level `SimAction`

The concrete simulator commands are the `SimAction` enum:

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
- `BuildGoal` starts the abstract goal project.
- `Assist` adds all specified idle engineers to an already-started project.
- `Wait` advances the simulator by one tick.

`Build`, `Upgrade`, and `BuildGoal` accept a `Vec<NodeId>` so that a squad of engineers can be committed immediately. The simulator assigns the total build power of all listed builders to the project.

## Execution in the one-step policy

The one-step planner ties the pieces together in `macro_policy_plan`:

```rust
// crates/faf-sim/src/planner/policy/direction_planner.rs ~line 44 — macro_policy_plan (abbreviated)
pub(crate) fn macro_policy_plan(
    units: &Units,
    mut state: SimulationState,
    goal: &Goal,
    policy_bundle: Option<&dyn ValueNet>,
    deterministic: bool,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    // 1. featurize the state
    // 2. run the direction head
    // 3. mask illegal directions and pick one
    // 4. heuristic converts direction to a concrete SimAction
    // 5. execute SimAction::Build/Upgrade/BuildGoal
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

The raw action space is combinatorial: many units, many builders, many timings. The direction-only design collapses it to a constant branching factor of six:

```text
state → state_features → direction head → 6 logits → masked softmax → direction
                                                ↓
                                        heuristic layer
                                                ↓
                                        concrete SimAction
```

This is `O(1)` forward work per decision at the network level. The heuristic scan of the plan graph is cheap because the plan graph is small and static. The reactive loop repeats this work every tick, but each decision is fast enough to keep up with the simulator.

## Legal move validation

Not every direction is valid in every state. The simulator rejects actions that violate constraints:

- A busy builder cannot start a new project.
- A builder cannot build or upgrade a unit it is not capable of building.
- A builder cannot upgrade a unit that has no registered upgrade target.
- A builder cannot assist a non-existent project.

The direction mask pre-filters illegal directions, and the heuristic plus the simulator's `start_project`/`start_upgrade_project` methods catch the rest. The planner handles rejection by issuing `Wait` and trying again on the next tick.

## Discrete actions, continuous time

Actions are discrete (build this unit, upgrade that unit, assist, wait), but the simulator advances by a fixed `dt` each tick. The planner therefore operates on a discretized decision grid. The choice of `dt` matters:

- A small `dt` gives finer control but issues more decisions.
- A large `dt` is faster but may miss tight timing windows.

The default `PlannerConfig::dt` is `1.0` second.

Now that we know what actions look like, we can build the Burn network that chooses the high-level directions.
