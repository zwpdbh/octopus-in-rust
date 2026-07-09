//! One-step direction-only policy planner.
//!
//! Implements the deterministic (or stochastic) inference path that uses the
//! learned direction network to pick a high-level strategic direction, then
//! delegates concrete action selection to the heuristic layer.

use crate::engine::{GraphSimError, Simulation};
use crate::planner::core::{Goal, PlanResult, PlannerConfig, PlannerError, ValueNetKind};
use crate::planner::plan_graph::{build_plan_graph, EdgeCategory, PlanGraph};
use crate::planner::policy::features::state_features;
use crate::planner::policy::heuristic::{direction_to_action, is_direction_legal};
use crate::planner::policy::macro_net::{
    masked_argmax, masked_sample_index, ECO_DIRECTION_INDICES, GOAL_DIRECTION_INDEX,
};
use crate::planner::policy::value_net::{MlpValueNet, ValueNet};
use crate::planner::SimAction;
use crate::units::Units;

/// Run the one-step direction-only policy from `initial_state` toward `goal`.
#[allow(clippy::too_many_arguments)]
pub fn plan(
    units: &Units,
    initial_state: Simulation,
    goal: &Goal,
    _iterations: usize,
    value_net_kind: ValueNetKind,
    deterministic: bool,
    policy_bundle: Option<&dyn ValueNet>,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    match value_net_kind {
        ValueNetKind::Mlp => macro_policy_plan(
            units,
            initial_state,
            goal,
            policy_bundle,
            deterministic,
            config,
        ),
        ValueNetKind::Gnn => Err(PlannerError::UnsupportedStrategy(
            "GNN value net is not yet implemented".to_string(),
        )),
    }
}

/// One-step planner guided by the direction-only policy network.
pub(crate) fn macro_policy_plan(
    units: &Units,
    mut state: Simulation,
    goal: &Goal,
    policy_bundle: Option<&dyn ValueNet>,
    deterministic: bool,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    let plan = build_plan_graph(units, *goal);

    let default_net;
    let bundle: &dyn ValueNet = match policy_bundle {
        Some(b) => b,
        None => {
            default_net = MlpValueNet::new();
            &default_net
        }
    };

    let features = state_features(&state, units, config);
    let eco_logits = bundle.evaluate_direction(features.clone());
    let rush_p = bundle.evaluate_rush(features);
    let direction_mask = legal_direction_mask(&state, units, config, goal, &plan);

    if direction_mask.iter().all(|&b| !b) {
        state.tick(config.dt);
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    // Build the eco-only mask (length 5) from the full direction mask (length 6).
    let eco_mask: Vec<bool> = ECO_DIRECTION_INDICES
        .iter()
        .map(|&i| direction_mask[i])
        .collect();

    let best_eco_idx = if deterministic {
        masked_argmax(&eco_logits, &eco_mask)
    } else {
        let mut rng = rand::rng();
        masked_sample_index(&eco_logits, &eco_mask, &mut rng)
    }
    .unwrap_or(0);

    let goal_legal = direction_mask[GOAL_DIRECTION_INDEX];
    let direction = if goal_legal && should_rush(deterministic, rush_p, config.rush_threshold) {
        EdgeCategory::Goal
    } else {
        EdgeCategory::ALL[ECO_DIRECTION_INDICES[best_eco_idx]]
    };

    let action = direction_to_action(direction, &state, units, config, goal, &plan);

    let mut new_state = state.clone();
    if execute_action(&mut new_state, &action, units, config.dt).is_err() {
        // Heuristic produced an infeasible action; fall back to wait.
        state.tick(config.dt);
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    Ok(plan_result_with_action(new_state, action))
}

/// Build a boolean mask over [`EdgeCategory::ALL`] indicating which directions
/// have at least one legal concrete action right now.
fn legal_direction_mask(
    state: &Simulation,
    units: &Units,
    config: &PlannerConfig,
    goal: &Goal,
    plan: &PlanGraph,
) -> Vec<bool> {
    EdgeCategory::ALL
        .iter()
        .map(|&d| is_direction_legal(d, state, units, config, goal, plan))
        .collect()
}

/// Apply a [`SimAction`] to a mutable simulator state.
pub(crate) fn execute_action(
    state: &mut Simulation,
    action: &SimAction,
    _units: &Units,
    dt: f64,
) -> Result<(), GraphSimError> {
    match action {
        SimAction::Build { unit_id, builders } => {
            state.start_project(unit_id, builders)?;
        }
        SimAction::BuildGoal { goal, builders } => {
            state.start_goal_project(*goal, builders)?;
        }
        SimAction::Upgrade {
            target_unit_id,
            old_node,
            builders,
        } => {
            state.start_upgrade_project(target_unit_id, *old_node, builders)?;
        }
        SimAction::Assist {
            project_node,
            builders,
        } => {
            if builders.is_empty() {
                return Ok(());
            }
            state.assist_project(*project_node, builders)?;
        }
        SimAction::Wait => {
            state.tick(dt);
        }
    }
    Ok(())
}

/// Decide whether to start the goal based on the rush head and planner mode.
fn should_rush(deterministic: bool, rush_p: f32, rush_threshold: f64) -> bool {
    if deterministic {
        rush_p >= rush_threshold as f32
    } else {
        rand::random::<f32>() < rush_p
    }
}

/// Build a [`PlanResult`] that commits to a single immediate action.
pub(crate) fn plan_result_with_action(state: Simulation, action: SimAction) -> PlanResult {
    PlanResult {
        events: Vec::new(),
        completion_time: state.graph.time,
        final_economy: state.engine.economy.clone(),
        first_action: Some(action),
        final_state: state,
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
        let state = Simulation::new(&[UnitKind::Commander], units.clone(), 10);
        let config = PlannerConfig::default();

        let goal = Goal {
            tech_level: crate::units::TechLevel::T4,
            mass_cost: 28_000.0,
            energy_cost: 340_000.0,
            build_time: 46_250.0,
        };
        let result = macro_policy_plan(&units, state, &goal, None, true, &config)
            .expect("plan should succeed");

        assert!(
            result.first_action.is_some(),
            "plan should return an action from the starting ACU state"
        );
    }
}
