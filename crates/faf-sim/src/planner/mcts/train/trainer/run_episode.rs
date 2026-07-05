//! Trainer episode generation for the direction-only policy network.

use rand::RngExt;

use super::super::episode::{Episode, EpisodeStep};
use super::super::reward::{compute_step_reward, compute_terminal_bonus, MilestoneTracker};

use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::mcts::direction_planner::execute_action;
use crate::planner::mcts::features::state_features;
use crate::planner::mcts::heuristic::{direction_to_action, is_direction_legal};
use crate::planner::mcts::macro_net::masked_sample_index;
use crate::planner::plan_graph::{build_plan_graph, EdgeCategory, PlanGraph};
use crate::sim::SimulationState;
use crate::units::{UnitKind, Units};

use super::Trainer;

impl Trainer {
    /// Run one episode and record the trajectory.
    pub(crate) fn run_episode(
        &mut self,
        _ep: usize,
        units: &Units,
        goal: &Goal,
        planner_config: &PlannerConfig,
        epsilon: f32,
    ) -> Episode {
        let plan = build_plan_graph(units, *goal);
        let mut state = SimulationState::new(units, &[UnitKind::Commander]);
        let mut episode = Episode {
            reached_goal: false,
            completion_time: 0.0,
            final_reward: 0.0,
            steps: Vec::new(),
        };
        let mut milestones = MilestoneTracker::default();

        for _step in 0..self.config.max_steps {
            if state.goal_reached(goal) {
                episode.reached_goal = true;
                episode.completion_time = state.time;
                break;
            }

            let base_features = state_features(&state, units, planner_config);

            let direction_mask = legal_direction_mask(&state, units, planner_config, goal, &plan);
            if direction_mask.iter().all(|&b| !b) {
                state.tick(units, self.config.dt);
                continue;
            }

            let direction_logits = self
                .model
                .evaluate_direction(base_features.clone(), &self.device);

            let direction_idx = if self.rng.random::<f32>() < epsilon {
                let legal_directions: Vec<usize> = direction_mask
                    .iter()
                    .enumerate()
                    .filter(|(_, &legal)| legal)
                    .map(|(i, _)| i)
                    .collect();
                *legal_directions
                    .get(self.rng.random_range(0..legal_directions.len()))
                    .unwrap_or(&0)
            } else {
                masked_sample_index(&direction_logits, &direction_mask, &mut self.rng).unwrap_or(0)
            };
            let direction = EdgeCategory::ALL[direction_idx];

            let action = direction_to_action(direction, &state, units, planner_config, goal, &plan);

            let prev_state = state.clone();
            if execute_action(&mut state, &action, units, self.config.dt).is_err() {
                state.tick(units, self.config.dt);
                continue;
            }

            let mut step_reward = compute_step_reward(&prev_state, &state, units);
            step_reward += milestones.update(&state, units);

            episode.steps.push(EpisodeStep {
                base_features,
                direction_mask,
                direction_index: direction_idx,
                step_reward,
                return_value: 0.0,
            });
        }

        episode.final_reward = compute_terminal_bonus(&state, episode.reached_goal);
        self.compute_returns(&mut episode);
        episode
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
    use crate::planner::mcts::macro_net::DIRECTION_COUNT;
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
