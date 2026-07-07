//! Trainer episode generation for the direction-only policy network.

use super::super::episode::{Episode, EpisodeStep};
use super::super::reward::compute_step_reward;

use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::plan_graph::{EdgeCategory, PlanGraph};
use crate::planner::policy::direction_planner::execute_action;
use crate::planner::policy::features::state_features;
use crate::planner::policy::heuristic::{direction_to_action, is_direction_legal};
use crate::planner::policy::macro_net::masked_argmax;
use crate::sim::SimulationState;
use crate::units::{UnitKind, Units};

use super::Trainer;

impl Trainer {
    /// Run one episode, applying an online policy-gradient update after each
    /// step. Returns the episode and the average step loss.
    pub(crate) fn run_episode(
        &mut self,
        units: &Units,
        goal: &Goal,
        planner_config: &PlannerConfig,
        plan: &PlanGraph,
    ) -> (Episode, f32) {
        let mut state = SimulationState::new(units, &[UnitKind::Commander]);
        let mut episode = Episode {
            reached_goal: false,
            completion_time: 0.0,
            steps: Vec::new(),
        };
        let mut accumulated_loss = 0.0f32;
        let mut step_count = 0usize;

        for _step in 0..self.config.max_steps {
            if state.goal_reached(goal) {
                episode.reached_goal = true;
                episode.completion_time = state.time;
                break;
            }

            let base_features = state_features(&state, units, planner_config);

            let direction_mask = legal_direction_mask(&state, units, planner_config, goal, plan);
            if direction_mask.iter().all(|&b| !b) {
                state.tick(units, self.config.dt);
                continue;
            }

            let direction_logits = self
                .model
                .evaluate_direction(base_features.clone(), &self.device);

            let direction_idx = masked_argmax(&direction_logits, &direction_mask).unwrap_or(0);
            let direction = EdgeCategory::ALL[direction_idx];

            let action = direction_to_action(direction, &state, units, planner_config, goal, plan);

            let prev_state = state.clone();
            if execute_action(&mut state, &action, units, self.config.dt).is_err() {
                state.tick(units, self.config.dt);
                continue;
            }

            let step_reward = compute_step_reward(&prev_state, &state, units, &self.config);
            let step = EpisodeStep {
                base_features,
                direction_mask,
                direction_index: direction_idx,
            };

            accumulated_loss += self.update_step(&step, step_reward);
            step_count += 1;
            episode.steps.push(step);
        }

        let avg_loss = if step_count == 0 {
            0.0
        } else {
            accumulated_loss / step_count as f32
        };
        (episode, avg_loss)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::plan_graph::build_plan_graph;
    use crate::planner::policy::macro_net::DIRECTION_COUNT;
    use crate::units::{TechLevel, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../../../plugins/faf-units/data/faf_units.json");
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

    #[test]
    fn legal_direction_mask_includes_mass_from_acu() {
        let units = load_units();
        let state = SimulationState::new(&units, &[UnitKind::Commander]);
        let config = PlannerConfig::default();
        let goal = t4_goal();
        let plan = build_plan_graph(&units, goal);

        let mask = legal_direction_mask(&state, &units, &config, &goal, &plan);
        assert_eq!(mask.len(), DIRECTION_COUNT);
        assert!(mask[EdgeCategory::IncreaseMass as usize]);
    }
}
