# 2. Actions and Successors

The MLP planner expands a state by choosing from the legal **candidates** derived from the `PlanGraph`. This chapter describes the two-level action model: high-level candidates used by the learned policy, and low-level simulator commands that actually mutate `GraphState`.

## Two-level action model

The planner makes decisions at the level of **candidates**:

```rust
// crates/faf-sim/src/planner/mcts/pools.rs ~line 21 — Candidate enum
pub enum Candidate {
    /// Build a new unit of the given kind.
    Build(UnitKind),
    /// Upgrade an existing `from` unit into `to`.
    Upgrade { from: UnitKind, to: UnitKind },
    /// Assign all idle engineers of the given tier to assist an active project.
    Assist(TechLevel),
}
```

A candidate is an abstract choice. Before it can be executed it is converted into a concrete `SearchAction`:

```rust
// crates/faf-sim/src/planner/mcts/mod.rs ~line 124 — candidate_to_action
pub(crate) fn candidate_to_action(
    candidate: &Candidate,
    state: &GraphState,
    units: &Units,
    _plan: &PlanGraph,
) -> Option<SearchAction> {
    // ...
}
```

The separation is useful because the MLP policy reasons over a small, plan-graph-constrained set of candidates, while the simulator still consumes the existing `SearchAction` commands.

## Generating candidates from the plan graph

`SelectionPools` derives the current legal candidates by walking the static `PlanGraph`:

```rust
// crates/faf-sim/src/planner/mcts/pools.rs ~line 70 — SelectionPools
pub struct SelectionPools {
    /// Units that can be built next.
    pub build: Vec<UnitKind>,
    /// Upgrades that can be started next.
    pub upgrade: Vec<(UnitKind, UnitKind)>,
    /// Idle engineers available to assist, grouped by tier.
    pub assist: AssistCounts,
}
```

```rust
// crates/faf-sim/src/planner/mcts/pools.rs ~line 82 — SelectionPools::derive
pub fn derive(plan: &PlanGraph, state: &GraphState, units: &Units) -> Self {
    let mut build = HashSet::new();
    let mut upgrade = HashSet::new();

    let active_targets = state.active_target_unit_ids();

    for edge in plan.graph().edge_references() {
        let source = &plan.graph()[edge.source()];
        let target = &plan.graph()[edge.target()];

        // Source must be owned and active; target must not be owned or
        // already under construction.
        if !state.has_completed_unit(source)
            || state.has_completed_unit(target)
            || active_targets.contains(target)
        {
            continue;
        }

        match edge.weight() {
            PlanEdgeKind::Build => {
                // Source in a build edge is the builder.
                if is_idle_builder(state, units, source) {
                    build.insert(target.clone());
                }
            }
            PlanEdgeKind::Upgrade => {
                // Source in an upgrade edge is the unit being upgraded.
                if can_upgrade(state, units, source, target) {
                    upgrade.insert((source.clone(), target.clone()));
                }
            }
        }
    }

    Self {
        build: build.into_iter().collect(),
        upgrade: upgrade.into_iter().collect(),
        assist: derive_assist_counts(state, units),
    }
}
```

For each edge `source -> target` in the plan graph:

- The source must be a completed, active unit in the current state.
- The target must not already be completed or under construction.
- For a **build** edge, the source must be an idle builder capable of building the target.
- For an **upgrade** edge, there must be a finished source unit and an idle builder capable of performing the upgrade.

`Assist` candidates are derived separately from idle engineers grouped by tech tier:

```rust
// crates/faf-sim/src/planner/mcts/pools.rs ~line 36 — AssistCounts
pub struct AssistCounts {
    pub t1: u32,
    pub t2: u32,
    pub t3: u32,
}
```

## From candidates to executable actions

`SelectionPools::candidates` flattens the pools into a `Vec<Candidate>`:

```rust
// crates/faf-sim/src/planner/mcts/pools.rs ~line 125 — SelectionPools::candidates
pub fn candidates(&self) -> Vec<Candidate> {
    // ... build -> Candidate::Build, upgrade -> Candidate::Upgrade,
    //     non-empty assist tiers -> Candidate::Assist
}
```

The policy scores each candidate, but not every scored candidate can be executed immediately. The planner filters by `candidate_to_action` before sampling. For example, a `Build` candidate needs a specific idle builder node; if the only capable builder became busy during the current tick, the candidate is skipped and the planner issues `Wait`.

## Low-level `SearchAction`

The concrete simulator commands are still the existing `SearchAction` enum:

```rust
// crates/faf-sim/src/planner/search.rs ~line 14 — SearchAction enum
pub enum SearchAction {
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
- `Upgrade` reuses an existing finished slot and transitions it to a higher-tier unit.
- `Assist` adds all idle engineers of a tier to an already-started project.
- `Wait` advances the simulator by one tick.

The current MLP planner usually assigns a single builder; future successors may assign multiple builders to a project.

## Why the branching factor matters

The candidate list is already much smaller than the raw successor list would be because the `PlanGraph` prunes away units that are not on the path to the goal. Even so, a state with several idle engineers and factories may have many legal candidates. The policy network keeps the decision cheap:

1. Generate candidates once with `SelectionPools::derive`.
2. Score all candidates in a single batched forward pass.
3. Sample one candidate and convert it to a `SearchAction`.

This is `O(n_candidates)` forward work per decision, not a tree expansion.

## Legal move validation

Not every `Candidate` is valid in every state. The simulator rejects actions that violate constraints:

- A busy builder cannot start a new project.
- A builder cannot build or upgrade a unit it is not capable of building.
- A builder cannot upgrade a unit that has no registered upgrade target.
- A builder cannot assist a non-existent project.

The candidate generator pre-filters most illegal actions, and `candidate_to_action` plus the simulator's `start_project`/`assist_project` methods catch the rest. The planner handles rejection by issuing `Wait` and trying again on the next tick.

## Key design choice: discrete actions, continuous time

Actions are discrete (build this unit, upgrade that unit, assist, wait), but time is continuous. The simulator advances by a fixed `dt` each tick. The planner therefore operates on a discretized decision grid. The choice of `dt` matters:

- A small `dt` gives finer control but issues more decisions.
- A large `dt` is faster but may miss tight timing windows.

The default `PlannerConfig::dt` is `10.0` seconds.
