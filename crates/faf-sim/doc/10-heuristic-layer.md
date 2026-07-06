# 10. The Heuristic Layer

The learned network only chooses a high-level direction. This chapter describes the deterministic heuristic layer that turns that direction into a concrete `SimAction`: which unit to build or upgrade, and which engineers to assign.

Keeping these rules explicit has three advantages:

1. **Interpretability.** You can read the heuristic and understand why a particular unit was chosen.
2. **Correctness.** Target selection and builder assignment are easy to verify with unit tests.
3. **Small network.** The policy only needs to learn strategic timing, not low-level build details.

## Entry point

`direction_to_action` dispatches to a per-direction helper:

```rust
// crates/faf-sim/src/planner/mcts/heuristic.rs ~line 25 — direction_to_action
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

If the chosen direction has no legal concrete action, the helper returns `SimAction::Wait`.

## Finding legal candidates

Each helper starts by scanning the plan graph for legal edges in its category:

```rust
// crates/faf-sim/src/planner/mcts/heuristic.rs ~line 66 — legal_candidates
fn legal_candidates(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
    category: EdgeCategory,
) -> Vec<Candidate> {
    let mut seen: std::collections::HashSet<Candidate> = std::collections::HashSet::new();
    let mut candidates = Vec::new();

    for edge in plan.graph().edge_references() {
        let action = *edge.weight();
        let source = &plan.graph()[edge.source()];
        let target = &plan.graph()[edge.target()];

        if EdgeCategory::categorize(action, target) != category {
            continue;
        }
        if !is_plan_edge_legal(action, source, target, state, units, config) {
            continue;
        }

        let candidate = match action {
            EdgeAction::Build => {
                let Some(target_kind) = target.as_unit() else { continue; };
                Candidate::Build { target: target_kind.clone() }
            }
            EdgeAction::Upgrade => {
                let Some(from) = source.as_unit() else { continue; };
                let Some(to) = target.as_unit() else { continue; };
                Candidate::Upgrade { from: from.clone(), to: to.clone() }
            }
        };

        if seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    }

    candidates
}
```

Results are deduplicated so the same target reachable from multiple builders appears only once.

## IncreaseMass: shortest payback time

When the network chooses `IncreaseMass`, the heuristic picks the mass action with the shortest payback time:

```rust
// crates/faf-sim/src/planner/mcts/heuristic.rs ~line 134 — pick_mass_action
fn pick_mass_action(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    let candidates: Vec<_> = legal_candidates(plan, state, units, EdgeCategory::IncreaseMass)
        .into_iter()
        .filter(|c| matches!(c.target(), UnitKind::Mex(_) | UnitKind::CapT2Mex | UnitKind::CapT3Mex))
        .collect();

    let Some(best) = candidates
        .into_iter()
        .filter_map(|c| {
            let (target, source) = match &c {
                Candidate::Build { target } => (target.clone(), None),
                Candidate::Upgrade { from, to } => (to.clone(), Some(from.clone())),
            };
            let cost = project_cost(units, &target, source.as_ref())?;
            let gain = mass_income_gain(units, &target, source.as_ref())?;
            if gain <= 0.0 { return None; }
            Some((target, source, cost.mass / gain))
        })
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
    else {
        return SimAction::Wait;
    };

    build_or_upgrade(best.0, best.1, state, units, config)
}
```

Payback time is `mass cost / mass income gain`. T1 mexes have the shortest payback, so the heuristic fills all mex slots with T1 before upgrading. This matches typical FAF play: the mex limit makes each slot valuable, and the cheapest filler is usually best.

## IncreaseEnergy: highest tech

Energy actions are simpler: build the highest-tech legal power generator or upgrade:

```rust
// crates/faf-sim/src/planner/mcts/heuristic.rs ~line 173 — pick_energy_action
fn pick_energy_action(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    let candidates: Vec<_> = legal_candidates(plan, state, units, EdgeCategory::IncreaseEnergy)
        .into_iter()
        .filter(|c| matches!(c.target(), UnitKind::Pgen(_)))
        .collect();

    let Some(best) = candidates
        .into_iter()
        .max_by(|a, b| pgen_tier(a.target()).cmp(&pgen_tier(b.target())))
    else {
        return SimAction::Wait;
    };

    let (target, source) = match best {
        Candidate::Build { target } => (target, None),
        Candidate::Upgrade { from, to } => (to, Some(from)),
    };
    build_or_upgrade(target, source, state, units, config)
}
```

Higher-tech pgens provide more energy income per unit of build power, so once the infrastructure exists, preferring them is usually correct.

## IncreaseBP: highest-tier engineer

`IncreaseBP` builds the highest-tier engineer that is currently buildable:

```rust
// crates/faf-sim/src/planner/mcts/heuristic.rs ~line 199 — pick_bp_action
fn pick_bp_action(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    let candidates: Vec<_> = legal_candidates(plan, state, units, EdgeCategory::IncreaseBP)
        .into_iter()
        .filter(|c| matches!(c, Candidate::Build { target } if matches!(target, UnitKind::Engineer(_))))
        .map(|c| match c {
            Candidate::Build { target } => target,
            Candidate::Upgrade { .. } => unreachable!("filtered to builds only"),
        })
        .collect();

    let Some(target) = candidates
        .into_iter()
        .max_by(|a, b| engineer_tier(a).cmp(&engineer_tier(b)))
    else {
        return SimAction::Wait;
    };

    let builders = assign_builders(target.clone(), state, units, config.dt);
    if builders.is_empty() {
        return SimAction::Wait;
    }
    SimAction::Build { unit_id: target, builders }
}
```

Factory upgrades are handled by `UpgradeTech`, so `IncreaseBP` focuses purely on engineers.

## IncreaseEnergyStorage and Goal

`IncreaseEnergyStorage` builds an energy storage if one is legal. `Goal` starts the abstract goal project with any available T3 engineers. Both rely on `assign_builders` for squad selection.

## UpgradeTech: lowest-tier factory first

Factory upgrades prefer the lowest-tier idle factory, so tech progression is staged:

```rust
// crates/faf-sim/src/planner/mcts/heuristic.rs ~line 281 — pick_upgrade_action
fn pick_upgrade_action(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    let candidates: Vec<_> = legal_candidates(plan, state, units, EdgeCategory::UpgradeTech)
        .into_iter()
        .filter_map(|c| match c {
            Candidate::Upgrade { from, to }
                if matches!(from, UnitKind::Factory(_)) && matches!(to, UnitKind::Factory(_)) =>
            {
                Some((from, to))
            }
            _ => None,
        })
        .collect();

    let Some((from, to)) = candidates
        .into_iter()
        .min_by(|a, b| factory_tier(&a.0).cmp(&factory_tier(&b.0)))
    else {
        return SimAction::Wait;
    };

    let Some(old_node) = find_upgrade_source(state, &from) else {
        return SimAction::Wait;
    };
    let builders = assign_upgrade_builders(&from, &to, state, units, config.dt);
    if builders.is_empty() {
        return SimAction::Wait;
    }
    SimAction::Upgrade { target_unit_id: to, old_node, builders }
}
```

## Builder assignment and stall prevention

Both builds and upgrades use the same builder-assignment pattern: collect capable idle builders, sort by build rate (highest first), and add them greedily until adding one more would cause a mass or energy stall within one tick.

```rust
// crates/faf-sim/src/planner/mcts/heuristic.rs ~line 355 — assign_builders
fn assign_builders(
    target: UnitKind,
    state: &SimulationState,
    units: &Units,
    dt: f64,
) -> Vec<NodeId> {
    let cost = match units.build_cost(&target) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut candidates: Vec<NodeId> = state
        .idle_builders(units)
        .into_iter()
        .filter(|&id| units.can_build(&state.graph[id].unit_id, &target))
        .collect();

    candidates.sort_by(|&a, &b| {
        let rate_a = units.def(&state.graph[a].unit_id).map(|d| d.build_rate()).unwrap_or(0.0);
        let rate_b = units.def(&state.graph[b].unit_id).map(|d| d.build_rate()).unwrap_or(0.0);
        rate_b.total_cmp(&rate_a)
    });

    greedy_with_stall_gate(candidates, &cost.to_target_stats(), state, units, dt)
}
```

The stall gate is the key safety mechanism:

```rust
// crates/faf-sim/src/planner/mcts/heuristic.rs ~line 424 — greedy_with_stall_gate
fn greedy_with_stall_gate(
    candidates: Vec<NodeId>,
    target_stats: &faf_units::BuildTargetStats,
    state: &SimulationState,
    units: &Units,
    dt: f64,
) -> Vec<NodeId> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut squad = Vec::new();
    for &candidate in &candidates {
        let trial = {
            let mut s = squad.clone();
            s.push(candidate);
            s
        };
        let power = total_build_power_of_nodes(&trial, state, units);
        if let Some(drain) = compute_drain(target_stats, RequestedBuildPower(power)) {
            let mass_ok = state.economy.mass_storage <= 0.0
                || drain.mass_per_second * dt <= state.economy.mass_storage;
            let energy_ok = state.economy.energy_storage <= 0.0
                || drain.energy_per_second * dt <= state.economy.energy_storage;
            if !mass_ok || !energy_ok {
                break;
            }
        }
        squad.push(candidate);
    }

    if squad.is_empty() && !candidates.is_empty() {
        squad.push(candidates[0]);
    }

    squad
}
```

The gate computes the resource drain of the trial squad. If the next builder would drain mass or energy storage below zero within one tick, it stops. If even a single builder would stall, the heuristic falls back to one builder anyway — the network should learn to avoid directions that consistently produce stalled actions.

## Why these rules?

- **Payback time for mass** keeps early economy growth cheap and efficient.
- **Highest-tech energy/BP** leverages the infrastructure the agent has already invested in.
- **Lowest-tier factory first** stages tech upgrades and avoids upgrading a T2 factory while a T1 factory is still idle.
- **Stall gate** prevents the heuristic from issuing actions that immediately stall the economy.

The network's job is to decide *when* each direction is appropriate; the heuristic handles the rest.
