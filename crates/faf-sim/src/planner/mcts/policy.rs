//! One-step direction-only policy planner.
//!
//! Implements the deterministic (or stochastic) inference path that uses the
//! learned direction network to pick a high-level strategic direction, then
//! delegates concrete action selection to the heuristic layer.

use crate::planner::core::{Goal, PlanResult, PlannerConfig, PlannerError, ValueNetKind};
use crate::planner::mcts::features::state_features_with_shortfall;
use crate::planner::mcts::heuristic::direction_to_action;
use crate::planner::mcts::macro_net::{masked_argmax, masked_sample_index};
use crate::planner::mcts::value_net::{MlpValueNet, ValueNet};
use crate::planner::plan_graph::EdgeCategory;
use crate::planner::SimAction;
use crate::sim::{GraphSimError, SimulationState};
use crate::units::Units;

/// Run the one-step direction-only policy from `initial_state` toward `goal`.
pub fn plan(
    units: &Units,
    initial_state: SimulationState,
    goal: &Goal,
    _iterations: usize,
    value_net_kind: ValueNetKind,
    deterministic: bool,
    policy_bundle: Option<&dyn ValueNet>,
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

/// One-step planner guided by the direction-only policy network.
pub(crate) fn macro_policy_plan(
    units: &Units,
    mut state: SimulationState,
    goal: &Goal,
    policy_bundle: Option<&dyn ValueNet>,
    deterministic: bool,
    shortfall: &mut [f32; 3],
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    let default_net;
    let bundle: &dyn ValueNet = match policy_bundle {
        Some(b) => b,
        None => {
            default_net = MlpValueNet::new();
            &default_net
        }
    };

    let features = state_features_with_shortfall(&state, units, config, *shortfall);
    let direction_logits = bundle.evaluate_direction(features);
    let direction_mask = legal_direction_mask(&state, units, config, goal);

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
    let direction = EdgeCategory::ALL[direction_idx];

    let action = match direction_to_action(direction, &state, units, config, goal) {
        Some(action) => action,
        None => {
            // The network chose a direction that is no longer executable. This
            // should be rare because we mask illegal directions, but races can
            // happen between masking and execution.
            state.tick(units, config.dt);
            return Ok(plan_result_with_action(state, SimAction::Wait));
        }
    };

    let mut new_state = state.clone();
    if execute_action(&mut new_state, &action, units, config.dt).is_err() {
        // Heuristic produced an infeasible action; fall back to wait.
        state.tick(units, config.dt);
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    *shortfall = compute_shortfall(&action, &state, &new_state, units);
    Ok(plan_result_with_action(new_state, action))
}

/// Build a boolean mask over [`EdgeCategory::ALL`] indicating which directions
/// have at least one legal concrete action right now.
fn legal_direction_mask(
    state: &SimulationState,
    units: &Units,
    config: &PlannerConfig,
    goal: &Goal,
) -> Vec<bool> {
    EdgeCategory::ALL
        .iter()
        .map(|&d| direction_to_action(d, state, units, config, goal).is_some())
        .collect()
}

/// Apply a [`SimAction`] to a mutable simulator state.
pub(crate) fn execute_action(
    state: &mut SimulationState,
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

/// Compute shortfall feedback for the next tick.
///
/// For the new direction-only design this is mostly a placeholder: the heuristic
/// either succeeds or emits `Wait`. We keep the shortfall vector so the macro
/// network still receives the same input shape, but values are usually zero.
fn compute_shortfall(
    _action: &SimAction,
    _before: &SimulationState,
    _after: &SimulationState,
    _units: &Units,
) -> [f32; 3] {
    [0.0f32; 3]
}

/// Build a [`PlanResult`] that commits to a single immediate action.
pub(crate) fn plan_result_with_action(state: SimulationState, action: SimAction) -> PlanResult {
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
        let state = SimulationState::new(&units, &[UnitKind::Commander]);
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
