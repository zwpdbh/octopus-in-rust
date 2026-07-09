//! Reward shaping for the standalone eco trainer.

use crate::engine::simulation_state::SimulationState;
use crate::planner::policy::train::config::TrainEcoConfig;

/// Compute the reward for a single eco-training step.
///
/// The reward encourages growing mass income and penalizes resource stalls.
pub(crate) fn compute_eco_step_reward(
    prev_state: &SimulationState,
    next_state: &SimulationState,
    config: &TrainEcoConfig,
) -> f32 {
    let mass_income_delta = (next_state.economy.net_mass_income.value()
        - prev_state.economy.net_mass_income.value()) as f32;

    let mut reward = mass_income_delta * config.reward_mass_income_coef;

    // Penalise stalls.
    if next_state.economy.energy_storage.value() <= 1e-6
        && next_state.economy.net_energy_income.value() < 0.0
    {
        reward -= config.energy_stall_penalty * config.dt as f32;
    }
    if next_state.economy.mass_storage.value() <= 1e-6
        && next_state.economy.net_mass_income.value() < 0.0
    {
        reward -= config.mass_stall_penalty * config.dt as f32;
    }

    reward
}

/// Bonus reward for reaching the target mass income.
pub(crate) fn eco_episode_bonus(final_state: &SimulationState, target_mass_income: f64) -> f32 {
    if final_state.economy.net_mass_income.value() >= target_mass_income {
        100.0
    } else {
        0.0
    }
}
