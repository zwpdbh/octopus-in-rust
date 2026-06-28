//! State and candidate featurization for the value network.
//!
//! Converts a variable-size [`GraphState`] and a concrete [`SelectionOption`] into a
//! fixed-size `Vec<f32>` that the MLP can consume.

use petgraph::algo::dijkstra;
use petgraph::graph::NodeIndex;

use crate::planner::core::PlannerConfig;
use crate::planner::mcts::selections::SelectionOption;
use crate::planner::plan_graph::PlanGraph;
use crate::sim::GraphState;
use crate::units::{TechLevel, UnitKind, Units};

/// Number of state features.
pub const STATE_FEATURE_COUNT: usize = 12;

/// Number of candidate features.
pub const CANDIDATE_FEATURE_COUNT: usize = 12;

/// Total number of features fed into the value network.
pub const FEATURE_COUNT: usize = STATE_FEATURE_COUNT + CANDIDATE_FEATURE_COUNT;

/// Convert a simulator state into a fixed-length feature vector.
pub fn state_features(
    state: &GraphState,
    _goal_id: &UnitKind,
    units: &Units,
    config: &PlannerConfig,
) -> Vec<f32> {
    let mut features = Vec::with_capacity(STATE_FEATURE_COUNT);

    let economy = &state.economy;
    features.push(clamp((economy.net_mass_income / 100.0) as f32));
    features.push(clamp((economy.net_energy_income / 1000.0) as f32));
    features.push(storage_ratio(
        economy.mass_storage,
        economy.mass_storage_cap,
    ));
    features.push(storage_ratio(
        economy.energy_storage,
        economy.energy_storage_cap,
    ));
    features.push(clamp(
        (state.total_active_build_power(units) / 100.0) as f32,
    ));
    features.push(clamp((state.time / 3600.0) as f32));
    features.push(clamp(
        state.count_active_mex() as f32 / config.max_mex_count as f32,
    ));
    features.push(clamp(
        state.count_active_pgen() as f32 / config.max_pgen_count as f32,
    ));
    let active_project_count = state
        .graph
        .graph
        .node_weights()
        .filter(|n| {
            matches!(
                n.state,
                crate::sim::UnitNodeState::Constructing { .. }
                    | crate::sim::UnitNodeState::Upgrading { .. }
            )
        })
        .count();
    features.push(clamp(active_project_count as f32 / 10.0));
    features.push(bool_f32(
        state.has_completed_unit(&UnitKind::Factory(TechLevel::T2)),
    ));
    features.push(bool_f32(
        state.has_completed_unit(&UnitKind::Factory(TechLevel::T3)),
    ));
    features.push(bool_f32(
        state.has_completed_unit(&UnitKind::Engineer(TechLevel::T3)),
    ));

    debug_assert_eq!(features.len(), STATE_FEATURE_COUNT);
    features
}

/// Convert a selection option into a fixed-length feature vector.
pub fn candidate_features(
    candidate: &SelectionOption,
    state: &GraphState,
    plan: &PlanGraph,
    units: &Units,
) -> Vec<f32> {
    let mut features = vec![0.0f32; CANDIDATE_FEATURE_COUNT];

    let (target_kind, is_build, is_upgrade, is_assist, builder_power) = match candidate {
        SelectionOption::Build(target) => (target, 1.0f32, 0.0f32, 0.0f32, 0.0f32),
        SelectionOption::Upgrade { to, .. } => (to, 0.0f32, 1.0f32, 0.0f32, 0.0f32),
        SelectionOption::Assist(target) => {
            let target_kind = &state.graph[*target].unit_id;
            let power = idle_engineer_power(state, units);
            (target_kind, 0.0f32, 0.0f32, 1.0f32, power)
        }
    };

    features[0] = is_build;
    features[1] = is_upgrade;
    features[2] = is_assist;
    features[3] = tier_value(tier_of(target_kind));
    features[4] = bool_f32(matches!(target_kind, UnitKind::Mex(_)));
    features[5] = bool_f32(matches!(target_kind, UnitKind::Pgen(_)));
    features[6] = bool_f32(matches!(target_kind, UnitKind::Factory(_)));
    features[7] = bool_f32(matches!(target_kind, UnitKind::Engineer(_)));
    features[8] = bool_f32(matches!(target_kind, UnitKind::Unique(_)));

    if is_assist > 0.0 {
        features[9] = clamp(builder_power / 100.0);
        features[10] = 0.0;
        features[11] = 0.0; // no distance for assist
    } else if let Some(cost) = units.build_cost(target_kind) {
        features[9] = clamp(cost.mass as f32 / 10_000.0);
        features[10] = clamp(cost.energy as f32 / 100_000.0);
        features[11] = clamp(distance_to_goal(plan, target_kind) as f32 / 10.0);
    }

    features
}

/// Total build power of all idle engineers.
fn idle_engineer_power(state: &GraphState, units: &Units) -> f32 {
    state
        .idle_builders(units)
        .iter()
        .filter(|&&id| matches!(state.graph[id].unit_id, UnitKind::Engineer(_)))
        .filter_map(|&id| units.def(&state.graph[id].unit_id))
        .map(|d| d.build_rate as f32)
        .sum()
}

/// Combine state and candidate features into one vector.
pub fn featurize(
    state: &GraphState,
    candidate: &SelectionOption,
    goal_id: &UnitKind,
    units: &Units,
    plan: &PlanGraph,
    config: &PlannerConfig,
) -> Vec<f32> {
    let mut features = state_features(state, goal_id, units, config);
    features.extend(candidate_features(candidate, state, plan, units));
    debug_assert_eq!(features.len(), FEATURE_COUNT);
    features
}

/// Clamp a value to a reasonable range and handle NaN.
fn clamp(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-10.0, 10.0)
    } else {
        0.0
    }
}

/// Convert a boolean to 0.0 or 1.0.
fn bool_f32(b: bool) -> f32 {
    if b {
        1.0
    } else {
        0.0
    }
}

/// Return the storage ratio, or 0.0 if capacity is zero.
fn storage_ratio(current: f64, cap: f64) -> f32 {
    if cap > 0.0 {
        clamp((current / cap) as f32)
    } else {
        0.0
    }
}

/// Extract the tech tier of a unit kind, if it has one.
fn tier_of(kind: &UnitKind) -> TechLevel {
    match kind {
        UnitKind::Engineer(t) | UnitKind::Factory(t) | UnitKind::Mex(t) | UnitKind::Pgen(t) => *t,
        UnitKind::Commander => TechLevel::T1,
        UnitKind::Unique(_) => TechLevel::T4,
    }
}

/// Normalize a tech tier to [0, 1].
fn tier_value(tier: TechLevel) -> f32 {
    match tier {
        TechLevel::T1 => 0.0,
        TechLevel::T2 => 0.33,
        TechLevel::T3 => 0.66,
        TechLevel::T4 => 1.0,
    }
}

/// Shortest number of edges from `kind` to the goal in the plan graph.
pub(crate) fn distance_to_goal(plan: &PlanGraph, kind: &UnitKind) -> usize {
    let goal_idx = plan
        .graph()
        .node_indices()
        .find(|i| plan.graph()[*i] == *plan.goal())
        .unwrap_or(NodeIndex::new(0));

    let distances = dijkstra(plan.graph(), goal_idx, None, |_| 1);

    plan.graph()
        .node_indices()
        .find(|i| plan.graph()[*i] == *kind)
        .and_then(|idx| distances.get(&idx).copied())
        .unwrap_or(usize::MAX / 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn feature_vector_has_expected_length() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let plan = units.plan_graph(&goal).unwrap();
        let state = GraphState::new(&units, &[UnitKind::Commander]);
        let config = PlannerConfig::default();
        let candidate = SelectionOption::Build(UnitKind::Mex(TechLevel::T1));

        let features = featurize(&state, &candidate, &goal, &units, &plan, &config);
        assert_eq!(features.len(), FEATURE_COUNT);
    }
}
