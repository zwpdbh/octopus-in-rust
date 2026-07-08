//! Trainer episode generation for the direction-only policy network.

use super::super::episode::{Episode, EpisodeStep};
use super::super::reward::compute_step_reward;
use super::super::rollout::{rush_rollout, RolloutResult};

use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::plan_graph::{EdgeCategory, PlanGraph};
use crate::planner::policy::direction_planner::execute_action;
use crate::planner::policy::features::state_features;
use crate::planner::policy::heuristic::{direction_to_action, is_direction_legal};
use crate::planner::policy::macro_net::{
    masked_argmax, ECO_DIRECTION_INDICES, GOAL_DIRECTION_INDEX,
};
use crate::sim::SimulationState;
use crate::units::{UnitKind, Units};

use super::Trainer;

impl Trainer {
    /// Run one episode, applying an online policy-gradient update after each
    /// step. Returns the episode and the average step loss.
    pub(crate) fn run_episode(
        &mut self,
        episode_idx: usize,
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

            let (eco_logits, rush_p) = self.model.evaluate(base_features.clone(), &self.device);

            let eco_mask: Vec<bool> = ECO_DIRECTION_INDICES
                .iter()
                .map(|&i| direction_mask[i])
                .collect();
            let best_eco_idx = masked_argmax(&eco_logits, &eco_mask).unwrap_or(0);

            let goal_legal = direction_mask[GOAL_DIRECTION_INDEX];
            let epsilon = self.current_epsilon(episode_idx);
            let direction = if goal_legal && self.should_explore_goal(epsilon, rush_p) {
                EdgeCategory::Goal
            } else if goal_legal && rush_p >= self.config.rush_threshold {
                EdgeCategory::Goal
            } else {
                EdgeCategory::ALL[ECO_DIRECTION_INDICES[best_eco_idx]]
            };
            let direction_idx = direction as usize;

            let action = direction_to_action(direction, &state, units, planner_config, goal, plan);

            let prev_state = state.clone();
            if execute_action(&mut state, &action, units, self.config.dt).is_err() {
                state.tick(units, self.config.dt);
                continue;
            }

            let (reward_eco, rush_target) = if direction == EdgeCategory::Goal {
                let rush_result = rush_rollout(&state, units, &self.config);
                let target = if rush_result.goal_finished { 1.0f32 } else { 0.0f32 };
                let reward = compute_step_reward(
                    &prev_state,
                    &state,
                    direction,
                    &action,
                    units,
                    &self.config,
                    goal,
                    rush_result,
                );
                (reward, target)
            } else {
                let rush_result = RolloutResult::default();
                let reward = compute_step_reward(
                    &prev_state,
                    &state,
                    direction,
                    &action,
                    units,
                    &self.config,
                    goal,
                    rush_result,
                );
                (reward, 0.0f32)
            };

            let step = EpisodeStep {
                base_features,
                direction_mask,
                direction_index: direction_idx,
                rush_p,
                rush_target,
            };

            accumulated_loss += self.update_step(&step, reward_eco);
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

impl Trainer {
    /// Current epsilon for Goal-only exploration, linearly decayed over episodes.
    fn current_epsilon(&self, episode_idx: usize) -> f32 {
        let start = self.config.epsilon_start;
        let end = self.config.epsilon_end;
        let decay_episodes = self.config.epsilon_decay_episodes.max(1);
        let progress = (episode_idx as f32 / decay_episodes as f32).min(1.0);
        start + (end - start) * progress
    }

    /// True when exploration forces a Goal pick regardless of the rush head.
    fn should_explore_goal(&self, epsilon: f32, _rush_p: f32) -> bool {
        rand::random::<f32>() < epsilon
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
        assert_eq!(mask.len(), EdgeCategory::ALL.len());
        assert!(mask[EdgeCategory::IncreaseMass as usize]);
    }
}
