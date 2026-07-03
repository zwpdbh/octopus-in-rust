//! Greedy evaluation for the direction-only policy network.

use super::super::{TrainBackend, TrainDevice};
use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::mcts::features::state_features;
use crate::planner::mcts::heuristic::{direction_to_action, is_direction_legal};
use crate::planner::mcts::macro_net::{masked_argmax, PolicyBundle};
use crate::planner::mcts::policy::execute_action;
use crate::planner::plan_graph::{build_plan_graph, EdgeCategory, PlanGraph};
use crate::sim::SimulationState;
use crate::units::{UnitKind, Units};

use super::Trainer;

impl Trainer {
    pub(crate) fn evaluate_greedy(
        &self,
        units: &Units,
        goal: &Goal,
        planner_config: &PlannerConfig,
    ) -> Option<f64> {
        Self::evaluate_greedy_with_model(
            &self.model,
            units,
            goal,
            planner_config,
            self.config.max_steps,
            self.config.dt,
            &self.device,
        )
    }

    pub(crate) fn evaluate_greedy_with_model(
        model: &PolicyBundle<TrainBackend>,
        units: &Units,
        goal: &Goal,
        planner_config: &PlannerConfig,
        max_steps: usize,
        dt: f64,
        device: &TrainDevice,
    ) -> Option<f64> {
        let plan = build_plan_graph(units, *goal);
        let mut state = SimulationState::new(units, &[UnitKind::Commander]);

        for _ in 0..max_steps {
            if state.goal_reached(goal) {
                return Some(state.time);
            }

            let features = state_features(&state, units, planner_config);
            let direction_logits = model.evaluate_direction(features, device);
            let direction_mask = legal_direction_mask(&state, units, planner_config, goal, &plan);

            if direction_mask.iter().all(|&b| !b) {
                state.tick(units, dt);
                continue;
            }

            let direction_idx = masked_argmax(&direction_logits, &direction_mask).unwrap_or(0);
            let direction = EdgeCategory::ALL[direction_idx];

            let action = direction_to_action(direction, &state, units, planner_config, goal, &plan);

            if execute_action(&mut state, &action, units, dt).is_err() {
                state.tick(units, dt);
            }
        }

        None
    }
}

/// Build a boolean mask over [`EdgeCategory::ALL`] indicating which directions
/// have at least one legal concrete action right now.
fn legal_direction_mask(
    state: &SimulationState,
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
