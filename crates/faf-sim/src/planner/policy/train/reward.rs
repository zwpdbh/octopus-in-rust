//! Reward shaping for policy-gradient training.

use crate::planner::policy::train::config::TrainConfig;
use crate::sim::SimulationState;
use crate::units::Units;

/// Compute the per-step reward.
///
/// For the first baseline we use only the change in mass income. A positive
/// delta means the chosen direction led to more mass generation; a negative
/// delta means mass income dropped. Other reward terms (build power, stalls,
/// milestones, terminal bonus) are removed so we can reason about a single
/// clean signal before adding complexity.
pub(crate) fn compute_step_reward(
    prev_state: &SimulationState,
    next_state: &SimulationState,
    _units: &Units, // unused for now; kept for API symmetry
    config: &TrainConfig,
) -> f32 {
    let prev_mass = prev_state.economy.net_mass_income as f32;
    let next_mass = next_state.economy.net_mass_income as f32;
    let mass_delta = (next_mass - prev_mass).clamp(-30.0, 30.0);
    (mass_delta * config.reward_mass_income_coef).clamp(-10.0, 10.0)
}
