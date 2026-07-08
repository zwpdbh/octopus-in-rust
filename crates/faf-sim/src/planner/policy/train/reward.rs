//! Rollout-based reward shaping for policy-gradient training.

use crate::planner::core::Goal;
use crate::planner::plan_graph::EdgeCategory;
use crate::planner::policy::train::config::TrainConfig;
use crate::planner::policy::train::rollout::{eco_rollout, RolloutResult};
use crate::planner::SimAction;
use crate::sim::SimulationState;
use crate::units::Units;

/// Compute the per-step reward using forward-looking rollouts.
///
/// For eco directions, the reward compares how much mass the economy can spend
/// on a phantom final-goal project before and after the action.
///
/// For the `Goal` direction, the reward comes from a real-goal rollout capped at
/// five minutes: finishing within the cap is strongly rewarded, failing to
/// finish is penalized.
pub(crate) fn compute_step_reward(
    prev_state: &SimulationState,
    next_state: &SimulationState,
    chosen_direction: EdgeCategory,
    _action: &SimAction,
    units: &Units,
    config: &TrainConfig,
    goal: &Goal,
    rush_result: RolloutResult,
) -> f32 {
    if chosen_direction == EdgeCategory::Goal {
        compute_rush_reward(&rush_result, config)
    } else {
        compute_eco_reward(prev_state, next_state, units, config, goal)
    }
}

fn compute_eco_reward(
    prev_state: &SimulationState,
    next_state: &SimulationState,
    units: &Units,
    config: &TrainConfig,
    goal: &Goal,
) -> f32 {
    let prev = eco_rollout(prev_state, units, config, goal);
    let next = eco_rollout(next_state, units, config, goal);

    let delta = next.mass_spent - prev.mass_spent;
    let mut reward = delta * config.mass_reward_coef;

    if delta <= 0.0 {
        reward -= config.wasted_action_penalty;
    }

    if next.mass_hoarded {
        reward -= config.hoarding_penalty;
    }

    if next.longest_energy_stall_secs > config.energy_stall_threshold_secs {
        reward -= config.stall_penalty;
    }

    reward
}

fn compute_rush_reward(result: &RolloutResult, config: &TrainConfig) -> f32 {
    let mut reward = 0.0f32;

    if result.goal_finished {
        let time_saved = config.rush_rollout_cap_secs - result.time_to_finish_secs.unwrap_or(0.0);
        reward += config.goal_finish_base_reward + time_saved * config.goal_time_reward_coef;
    } else {
        reward += config.goal_too_early_penalty;
    }

    if result.longest_energy_stall_secs > config.energy_stall_threshold_secs {
        reward -= config.stall_penalty;
    }

    reward
}
