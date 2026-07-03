//! Heuristic action layer for the direction-only policy network.
//!
//! The network now chooses only a high-level strategic direction. This module
//! turns that direction into a concrete [`SimAction`] by picking the best legal
//! target and assigning a greedy high-tech builder squad.

use crate::economy::{compute_drain, RequestedBuildPower};
use crate::planner::core::Goal;
use crate::planner::plan_graph::{
    find_upgrade_source, is_plan_edge_legal, EdgeAction, EdgeCategory, PlanGraph,
};
use crate::planner::SimAction;
use crate::sim::{NodeId, SimulationState};
use crate::units::{TechLevel, UnitKind, Units};
use petgraph::visit::EdgeRef;

/// Turn a network direction into a concrete simulator action.
///
/// Returns [`SimAction::Wait`] if the direction has no legal execution right now.
/// The caller can execute the returned action directly; there is no separate
/// "illegal direction" signal.
///
/// The caller must supply a pre-built [`PlanGraph`] for the current `units` and
/// `goal`; this avoids rebuilding the static plan graph on every call.
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

/// A build/upgrade target extracted from a legal plan-graph edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Candidate {
    target: UnitKind,
    /// For upgrades, the unit kind being upgraded.
    upgrade_from: Option<UnitKind>,
}

/// Return all legal build/upgrade candidates in `plan` whose category matches.
///
/// Results are deduplicated by `(target, source)` so multiple builders for the
/// same target appear only once.
fn legal_candidates(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    category: EdgeCategory,
) -> Vec<Candidate> {
    let mut seen: std::collections::HashSet<(UnitKind, Option<UnitKind>)> =
        std::collections::HashSet::new();
    let mut candidates = Vec::new();

    for edge in plan.graph().edge_references() {
        let action = *edge.weight();
        let source = &plan.graph()[edge.source()];
        let target = &plan.graph()[edge.target()];

        if EdgeCategory::categorize(action, target) != category {
            continue;
        }
        if !is_plan_edge_legal(action, source, target, state, units) {
            continue;
        }

        let Some(target_kind) = target.as_unit() else {
            continue;
        };
        let source_kind = if action == EdgeAction::Upgrade {
            source.as_unit().cloned()
        } else {
            None
        };

        let key = (target_kind.clone(), source_kind.clone());
        if seen.insert(key) {
            candidates.push(Candidate {
                target: target_kind.clone(),
                upgrade_from: source_kind,
            });
        }
    }

    candidates
}

/// True if `direction` has at least one legal concrete action in `state`.
pub fn is_direction_legal(
    direction: EdgeCategory,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
    goal: &Goal,
    plan: &crate::planner::plan_graph::PlanGraph,
) -> bool {
    !matches!(
        direction_to_action(direction, state, units, config, goal, plan),
        SimAction::Wait
    )
}

/// Pick the mass action with the shortest payback time.
fn pick_mass_action(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    let candidates: Vec<_> = legal_candidates(plan, state, units, EdgeCategory::IncreaseMass)
        .into_iter()
        .filter(|c| {
            matches!(
                c.target,
                UnitKind::Mex(_) | UnitKind::CapT2Mex | UnitKind::CapT3Mex
            )
        })
        .map(|c| (c.target, c.upgrade_from))
        .collect();

    let Some(best) = candidates
        .into_iter()
        .filter_map(|(target, source)| {
            let cost = project_cost(units, &target, source.as_ref())?;
            let gain = mass_income_gain(units, &target, source.as_ref())?;
            if gain <= 0.0 {
                return None;
            }
            Some((target, source, cost.mass / gain))
        })
        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
    else {
        return SimAction::Wait;
    };

    build_or_upgrade(best.0, best.1, state, units, config)
}

/// Pick the highest-tech legal energy action.
fn pick_energy_action(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    let candidates: Vec<_> = legal_candidates(plan, state, units, EdgeCategory::IncreaseEnergy)
        .into_iter()
        .filter(|c| matches!(c.target, UnitKind::Pgen(_)))
        .map(|c| (c.target, c.upgrade_from))
        .collect();

    let Some(best) = candidates
        .into_iter()
        .max_by(|a, b| pgen_tier(&a.0).cmp(&pgen_tier(&b.0)))
    else {
        return SimAction::Wait;
    };

    build_or_upgrade(best.0, best.1, state, units, config)
}

/// Pick the highest-tier engineer build action.
fn pick_bp_action(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    let candidates: Vec<_> = legal_candidates(plan, state, units, EdgeCategory::IncreaseBP)
        .into_iter()
        .filter(|c| matches!(c.target, UnitKind::Engineer(_)) && c.upgrade_from.is_none())
        .map(|c| c.target)
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
    SimAction::Build {
        unit_id: target,
        builders,
    }
}

/// Build energy storage if legal.
fn pick_storage_action(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    let can_build = legal_candidates(plan, state, units, EdgeCategory::IncreaseEnergyStorage)
        .iter()
        .any(|c| c.target == UnitKind::EnergyStorage && c.upgrade_from.is_none());

    if !can_build {
        return SimAction::Wait;
    }

    let builders = assign_builders(UnitKind::EnergyStorage, state, units, config.dt);
    if builders.is_empty() {
        return SimAction::Wait;
    }
    SimAction::Build {
        unit_id: UnitKind::EnergyStorage,
        builders,
    }
}

/// Start the abstract goal with T3 engineers.
fn pick_goal_action(
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
    goal: &Goal,
) -> SimAction {
    if state.goal_reached(goal) || state.goal_project_active() {
        return SimAction::Wait;
    }

    let target = UnitKind::Engineer(TechLevel::T3);
    let builders = assign_builders(target, state, units, config.dt);
    if builders.is_empty() {
        return SimAction::Wait;
    }
    SimAction::BuildGoal {
        goal: *goal,
        builders,
    }
}

/// Pick the lowest-tier legal factory upgrade.
fn pick_upgrade_action(
    plan: &PlanGraph,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    let candidates: Vec<_> = legal_candidates(plan, state, units, EdgeCategory::UpgradeTech)
        .into_iter()
        .filter_map(|c| match (c.upgrade_from, c.target) {
            (Some(from), to)
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
    SimAction::Upgrade {
        target_unit_id: to,
        old_node,
        builders,
    }
}

/// Helper: build or upgrade a target, returning the matching `SimAction`.
fn build_or_upgrade(
    target: UnitKind,
    source: Option<UnitKind>,
    state: &SimulationState,
    units: &Units,
    config: &crate::planner::core::PlannerConfig,
) -> SimAction {
    if let Some(from) = source {
        let Some(old_node) = find_upgrade_source(state, &from) else {
            return SimAction::Wait;
        };
        let builders = assign_upgrade_builders(&from, &target, state, units, config.dt);
        if builders.is_empty() {
            return SimAction::Wait;
        }
        SimAction::Upgrade {
            target_unit_id: target,
            old_node,
            builders,
        }
    } else {
        let builders = assign_builders(target.clone(), state, units, config.dt);
        if builders.is_empty() {
            return SimAction::Wait;
        }
        SimAction::Build {
            unit_id: target,
            builders,
        }
    }
}

/// Assign capable idle builders to a build target, high-tech first, with a hard
/// stall gate.
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
        let rate_a = units
            .def(&state.graph[a].unit_id)
            .map(|d| d.build_rate())
            .unwrap_or(0.0);
        let rate_b = units
            .def(&state.graph[b].unit_id)
            .map(|d| d.build_rate())
            .unwrap_or(0.0);
        rate_b.total_cmp(&rate_a)
    });

    greedy_with_stall_gate(candidates, &cost.to_target_stats(), state, units, dt)
}

/// Assign capable idle builders to an upgrade target, high-tech first, with a
/// hard stall gate.
fn assign_upgrade_builders(
    from: &UnitKind,
    to: &UnitKind,
    state: &SimulationState,
    units: &Units,
    dt: f64,
) -> Vec<NodeId> {
    let recipe = match units.upgrade_recipes(from).iter().find(|r| r.to == *to) {
        Some(r) => r,
        None => return Vec::new(),
    };

    let mut candidates: Vec<NodeId> = state
        .idle_builders(units)
        .into_iter()
        .filter(|&id| recipe.builder_options.contains(&state.graph[id].unit_id))
        .collect();

    candidates.sort_by(|&a, &b| {
        let rate_a = units
            .def(&state.graph[a].unit_id)
            .map(|d| d.build_rate())
            .unwrap_or(0.0);
        let rate_b = units
            .def(&state.graph[b].unit_id)
            .map(|d| d.build_rate())
            .unwrap_or(0.0);
        rate_b.total_cmp(&rate_a)
    });

    greedy_with_stall_gate(candidates, &recipe.cost.to_target_stats(), state, units, dt)
}

/// Greedily add builders until the next one would drive storage to zero within
/// one tick. Always keeps at least one builder if any candidate is legal.
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
                // Stall gate triggered; stop adding builders.
                break;
            }
        }
        squad.push(candidate);
    }

    // If no builder was added because even the first one fails the gate, fall
    // back to a single builder so the caller can still attempt the action. This
    // matches the rule "assign all capable idle builders, but check to prevent
    // stall": if there is no non-stalling assignment, we try the smallest one.
    if squad.is_empty() && !candidates.is_empty() {
        squad.push(candidates[0]);
    }

    squad
}

fn total_build_power_of_nodes(nodes: &[NodeId], state: &SimulationState, units: &Units) -> f64 {
    nodes
        .iter()
        .filter_map(|&id| units.def(&state.graph[id].unit_id))
        .map(|d| d.build_rate())
        .sum()
}

fn project_cost(
    units: &Units,
    target: &UnitKind,
    source: Option<&UnitKind>,
) -> Option<crate::units::UnitCost> {
    match source {
        Some(from) => units
            .upgrade_recipes(from)
            .iter()
            .find(|r| r.to == *target)
            .map(|r| r.cost),
        None => units.build_cost(target),
    }
}

fn mass_income_gain(units: &Units, target: &UnitKind, source: Option<&UnitKind>) -> Option<f64> {
    let target_income = units.def(target)?.mass_income();
    let source_income = source
        .and_then(|s| units.def(s))
        .map_or(0.0, |d| d.mass_income());
    Some(target_income - source_income)
}

fn engineer_tier(kind: &UnitKind) -> u8 {
    match kind {
        UnitKind::Engineer(TechLevel::T1) => 1,
        UnitKind::Engineer(TechLevel::T2) => 2,
        UnitKind::Engineer(TechLevel::T3) => 3,
        _ => 0,
    }
}

fn factory_tier(kind: &UnitKind) -> u8 {
    match kind {
        UnitKind::Factory(TechLevel::T1) => 1,
        UnitKind::Factory(TechLevel::T2) => 2,
        UnitKind::Factory(TechLevel::T3) => 3,
        _ => 0,
    }
}

fn pgen_tier(kind: &UnitKind) -> u8 {
    match kind {
        UnitKind::Pgen(TechLevel::T1) => 1,
        UnitKind::Pgen(TechLevel::T2) => 2,
        UnitKind::Pgen(TechLevel::T3) => 3,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::core::Goal;
    use crate::units::{TechLevel, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    fn t4_goal() -> Goal {
        Goal {
            tech_level: TechLevel::T4,
            mass_cost: 28_000.0,
            energy_cost: 340_000.0,
            build_time: 46_250.0,
        }
    }

    fn build_plan(units: &Units, goal: Goal) -> crate::planner::plan_graph::PlanGraph {
        crate::planner::plan_graph::build_plan_graph(units, goal)
    }

    #[test]
    fn mass_direction_builds_t1_mex_from_acu() {
        let units = load_units();
        let state = SimulationState::new(&units, &[UnitKind::Commander]);
        let config = crate::planner::core::PlannerConfig::default();
        let goal = t4_goal();
        let plan = build_plan(&units, goal);

        let action = direction_to_action(
            EdgeCategory::IncreaseMass,
            &state,
            &units,
            &config,
            &goal,
            &plan,
        );
        assert!(
            matches!(action, SimAction::Build { unit_id, .. } if unit_id == UnitKind::Mex(TechLevel::T1)),
            "ACU should build a T1 mex as the shortest-payback mass option"
        );
    }

    #[test]
    fn bp_direction_prefers_highest_tier_engineer() {
        let units = load_units();
        let state = SimulationState::new(
            &units,
            &[
                UnitKind::Commander,
                UnitKind::Factory(TechLevel::T1),
                UnitKind::Factory(TechLevel::T2),
                UnitKind::Factory(TechLevel::T3),
            ],
        );
        let config = crate::planner::core::PlannerConfig::default();
        let goal = t4_goal();
        let plan = build_plan(&units, goal);

        let action = direction_to_action(
            EdgeCategory::IncreaseBP,
            &state,
            &units,
            &config,
            &goal,
            &plan,
        );
        assert!(
            matches!(action, SimAction::Build { unit_id, .. } if unit_id == UnitKind::Engineer(TechLevel::T3)),
            "IncreaseBP should build the highest-tier engineer"
        );
    }

    #[test]
    fn upgrade_direction_prefers_lower_tier_factory() {
        let units = load_units();
        let state = SimulationState::new(
            &units,
            &[
                UnitKind::Commander,
                UnitKind::Factory(TechLevel::T1),
                UnitKind::Factory(TechLevel::T2),
            ],
        );
        let config = crate::planner::core::PlannerConfig::default();
        let goal = t4_goal();
        let plan = build_plan(&units, goal);

        let action = direction_to_action(
            EdgeCategory::UpgradeTech,
            &state,
            &units,
            &config,
            &goal,
            &plan,
        );
        let actual = match &action {
            SimAction::Upgrade { target_unit_id, .. } => target_unit_id.clone(),
            SimAction::Build { unit_id, .. } => unit_id.clone(),
            _ => UnitKind::Commander,
        };
        assert_eq!(
            actual,
            UnitKind::Factory(TechLevel::T2),
            "UpgradeTech should prefer T1->T2 over T2->T3, got {:?}",
            action
        );
    }
}
