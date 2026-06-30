//! One-step hierarchical policy planner.
//!
//! Implements the deterministic (or stochastic) inference path that uses the
//! three learned networks to pick a concrete plan-graph edge, a target build
//! power, and a [T1, T2, T3] engineer squad.

use burn::tensor::Device;

use crate::planner::core::{Goal, PlanResult, PlannerConfig, PlannerError, ValueNetKind};
use crate::planner::search::SimAction;
use crate::sim::{GraphSimError, GraphState, NodeId};
use crate::units::Units;

use super::features::state_features_with_shortfall;
use super::macro_net::{
    clamp_squad, ensure_minimum_squad, masked_argmax, masked_sample_index, plan_edge_index,
    shortfall_from_counts, PolicyBundle,
};
use super::selections::{
    assigned_squad_counts, find_upgrade_source, idle_engineer_counts, select_squad_for_edge,
};
use super::train::TrainBackend;
use crate::planner::plan_graph::EdgeCategory;

/// Run the one-step hierarchical policy from `initial_state` toward `goal_id`.
pub fn plan(
    units: &Units,
    initial_state: GraphState,
    goal: &Goal,
    _iterations: usize,
    value_net_kind: ValueNetKind,
    deterministic: bool,
    policy_bundle: Option<PolicyBundle<TrainBackend>>,
    shortfall: &mut [f32; 3],
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    match value_net_kind {
        ValueNetKind::Mlp => macro_policy_plan(
            units,
            initial_state,
            goal,
            policy_bundle,
            deterministic,
            shortfall,
            config,
        ),
        ValueNetKind::Gnn => Err(PlannerError::UnsupportedStrategy(
            "GNN value net is not yet implemented".to_string(),
        )),
    }
}

/// One-step planner guided by the hierarchical policy networks.
pub(crate) fn macro_policy_plan(
    units: &Units,
    mut state: GraphState,
    goal: &Goal,
    policy_bundle: Option<PolicyBundle<TrainBackend>>,
    deterministic: bool,
    shortfall: &mut [f32; 3],
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    let edge_index = plan_edge_index(units, goal)
        .ok_or_else(|| PlannerError::UnsupportedStrategy("goal has no plan graph".to_string()))?;

    let device: Device<TrainBackend> = Default::default();
    let bundle: PolicyBundle<TrainBackend> =
        policy_bundle.unwrap_or_else(|| PolicyBundle::new(&device, edge_index.len()));

    let macro_features = state_features_with_shortfall(&state, units, config, *shortfall);
    let direction_logits = bundle.evaluate_direction(macro_features.clone(), &device);
    let direction_mask = edge_index.legal_category_mask(&state, units, config);

    if direction_mask.iter().all(|&b| !b) {
        state.tick(units, config.dt);
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    let direction_idx = if deterministic {
        masked_argmax(&direction_logits, &direction_mask)
    } else {
        let mut rng = rand::rng();
        masked_sample_index(&direction_logits, &direction_mask, &mut rng)
    }
    .unwrap_or(0);
    let category = EdgeCategory::ALL[direction_idx];

    let action_logits = bundle.evaluate_action(macro_features.clone(), category, &device);
    let action_mask = edge_index.legal_mask_for_category(&state, units, config, category);

    if action_mask.iter().all(|&b| !b) {
        state.tick(units, config.dt);
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    let edge_idx = if deterministic {
        masked_argmax(&action_logits, &action_mask)
    } else {
        let mut rng = rand::rng();
        masked_sample_index(&action_logits, &action_mask, &mut rng)
    }
    .unwrap_or(0);

    let edge = match edge_index.get(edge_idx) {
        Some(e) => e.clone(),
        None => {
            state.tick(units, config.dt);
            return Ok(plan_result_with_action(state, SimAction::Wait));
        }
    };

    let power_mean =
        bundle.evaluate_power(macro_features.clone(), edge_idx, edge_index.len(), &device);
    let target_power = power_mean.max(0.0).round();

    let squad_raw = bundle.evaluate_squad(macro_features, target_power, &device);
    let squad_raw_arr = [
        squad_raw.get(0).copied().unwrap_or(0.0),
        squad_raw.get(1).copied().unwrap_or(0.0),
        squad_raw.get(2).copied().unwrap_or(0.0),
    ];

    let available = idle_engineer_counts(&state, units);
    let mut desired = clamp_squad(squad_raw_arr, available);
    desired = ensure_minimum_squad(desired, available);

    let builders = select_squad_for_edge(&edge, desired, &state, units);
    if builders.is_empty() {
        *shortfall = shortfall_from_counts(desired, available);
        state.tick(units, config.dt);
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    let assigned_counts = assigned_squad_counts(&state, &builders);
    let action = match edge.kind {
        crate::planner::plan_graph::PlanEdgeKind::Build => {
            if let Some(target_goal) = edge.target_goal() {
                SimAction::BuildGoal {
                    goal: *target_goal,
                    builders: builders.clone(),
                }
            } else {
                SimAction::Build {
                    unit_id: edge.target_unit().expect("build target unit").clone(),
                    builders: builders.clone(),
                }
            }
        }
        crate::planner::plan_graph::PlanEdgeKind::Upgrade => SimAction::Upgrade {
            target_unit_id: edge.target_unit().expect("upgrade target unit").clone(),
            old_node: find_upgrade_source(&state, edge.source_unit().expect("upgrade source unit"))
                .unwrap_or_else(|| NodeId::new(0)),
            builders: builders.clone(),
        },
    };

    if execute_action(&mut state, &action, units, config.dt).is_err() {
        *shortfall = shortfall_from_counts(desired, available);
        state.tick(units, config.dt);
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    *shortfall = shortfall_from_counts(desired, assigned_counts);
    Ok(plan_result_with_action(state, action))
}

/// Apply a [`SimAction`] to a mutable simulator state.
pub(crate) fn execute_action(
    state: &mut GraphState,
    action: &SimAction,
    units: &Units,
    dt: f64,
) -> Result<(), GraphSimError> {
    match action {
        SimAction::Build { unit_id, builders } => {
            state.start_project(unit_id, builders, units)?;
        }
        SimAction::BuildGoal { goal, builders } => {
            state.start_goal_project(*goal, builders, units)?;
        }
        SimAction::Upgrade {
            target_unit_id,
            old_node,
            builders,
        } => {
            state.start_upgrade_project(target_unit_id, *old_node, builders, units)?;
        }
        SimAction::Assist {
            project_node,
            builders,
        } => {
            if builders.is_empty() {
                return Ok(());
            }
            state.assist_project(*project_node, builders, units)?;
        }
        SimAction::Wait => {
            state.tick(units, dt);
        }
    }
    Ok(())
}

/// Build a [`PlanResult`] that commits to a single immediate action.
fn plan_result_with_action(state: GraphState, action: SimAction) -> PlanResult {
    PlanResult {
        events: Vec::new(),
        completion_time: state.time,
        final_economy: state.economy,
        first_action: Some(action),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{UnitKind, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn macro_plan_selects_build_action_from_acu() {
        let units = load_units();
        let state = GraphState::new(&units, &[UnitKind::Commander]);
        let config = PlannerConfig::default();
        let mut shortfall = [0.0f32; 3];

        let goal = Goal {
            tech_level: crate::units::TechLevel::T4,
            mass_cost: 28_000.0,
            energy_cost: 340_000.0,
            build_time: 46_250.0,
        };
        let result = macro_policy_plan(&units, state, &goal, None, true, &mut shortfall, &config)
            .expect("plan should succeed");

        assert!(
            result.first_action.is_some(),
            "plan should return an action from the starting ACU state"
        );
    }
}
