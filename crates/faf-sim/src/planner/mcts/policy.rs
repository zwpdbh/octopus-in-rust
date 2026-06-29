//! One-step hierarchical policy planner.
//!
//! Implements the deterministic (or stochastic) inference path that uses the
//! three learned networks to pick a concrete plan-graph edge, a target build
//! power, and a [T1, T2, T3] engineer squad.

use rand::prelude::*;

use burn::tensor::Device;

use crate::planner::core::{PlanResult, PlannerConfig, PlannerError, ValueNetKind};
use crate::planner::search::SimAction;
use crate::sim::{GraphSimError, GraphState, NodeId};
use crate::units::{UnitKind, Units};

use super::features::{state_features, state_features_with_shortfall};
use super::macro_net::{
    clamp_squad, ensure_minimum_squad, masked_argmax, masked_sample_index, one_hot,
    plan_edge_index, shortfall_from_counts, PolicyBundle,
};
use super::selections::{
    assigned_squad_counts, find_upgrade_source, idle_engineer_counts, select_squad_for_edge,
};
use super::train::TrainBackend;

/// Run the one-step hierarchical policy from `initial_state` toward `goal_id`.
pub fn plan(
    units: &Units,
    initial_state: GraphState,
    goal_id: &UnitKind,
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
            goal_id,
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
fn macro_policy_plan(
    units: &Units,
    mut state: GraphState,
    goal_id: &UnitKind,
    policy_bundle: Option<PolicyBundle<TrainBackend>>,
    deterministic: bool,
    shortfall: &mut [f32; 3],
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    let _plan = units
        .plan_graph(goal_id)
        .map_err(|e| PlannerError::UnsupportedStrategy(e.to_string()))?;
    let edge_index = plan_edge_index(units, goal_id)
        .ok_or_else(|| PlannerError::UnsupportedStrategy("goal has no plan graph".to_string()))?;

    let device: Device<TrainBackend> = Default::default();
    let bundle: PolicyBundle<TrainBackend> =
        policy_bundle.unwrap_or_else(|| PolicyBundle::new(&device, edge_index.len()));

    let base_features = state_features(&state, units, config);
    let macro_features = state_features_with_shortfall(&state, units, config, *shortfall);
    let macro_logits = bundle.macro_net.evaluate_single(macro_features, &device);
    let legal_mask = edge_index.legal_mask(&state, units, config);

    if legal_mask.iter().all(|&b| !b) {
        state.tick(units, config.dt);
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    let edge_idx = if deterministic {
        masked_argmax(&macro_logits, &legal_mask)
    } else {
        let mut rng = thread_rng();
        masked_sample_index(&macro_logits, &legal_mask, &mut rng)
    }
    .unwrap_or(0);

    let edge = match edge_index.get(edge_idx) {
        Some(e) => e.clone(),
        None => {
            state.tick(units, config.dt);
            return Ok(plan_result_with_action(state, SimAction::Wait));
        }
    };

    let power_features: Vec<f32> = base_features
        .iter()
        .copied()
        .chain(one_hot(edge_idx, edge_index.len()).into_iter())
        .collect();
    let power_mean = bundle.power_net.evaluate_single(power_features, &device)[0];
    let target_power = power_mean.max(0.0).round();

    let squad_features: Vec<f32> = base_features
        .iter()
        .copied()
        .chain(std::iter::once(target_power))
        .collect();
    let squad_raw = bundle.squad_net.evaluate_single(squad_features, &device);
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
        crate::planner::plan_graph::PlanEdgeKind::Build => SimAction::Build {
            unit_id: edge.target.clone(),
            builders: builders.clone(),
        },
        crate::planner::plan_graph::PlanEdgeKind::Upgrade => SimAction::Upgrade {
            target_unit_id: edge.target.clone(),
            old_node: find_upgrade_source(&state, &edge.source).unwrap_or_else(|| NodeId::new(0)),
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
    use crate::units::{UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn macro_plan_selects_build_action_from_acu() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let state = GraphState::new(&units, &[UnitKind::Commander]);
        let config = PlannerConfig::default();
        let mut shortfall = [0.0f32; 3];

        let result = macro_policy_plan(&units, state, &goal, None, true, &mut shortfall, &config)
            .expect("plan should succeed");

        assert!(
            result.first_action.is_some(),
            "plan should return an action from the starting ACU state"
        );
    }
}
