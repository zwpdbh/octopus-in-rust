//! Reward shaping for policy-gradient training.

use crate::sim::GraphState;
use crate::units::Units;

/// Compute a terminal bonus for reaching the goal.
///
/// A large positive reward encourages finishing, while a small time penalty
/// discourages very slow completions.
pub(crate) fn compute_terminal_bonus(state: &GraphState, goal_reached: bool) -> f32 {
    if goal_reached {
        1000.0 - state.time as f32 * 0.2
    } else {
        0.0
    }
}

/// Compute the per-step reward.
///
/// The agent is rewarded for increasing total active build power and punished
/// for resource stalls and mass overflow.
pub(crate) fn compute_step_reward(
    prev_state: &GraphState,
    next_state: &GraphState,
    units: &Units,
) -> f32 {
    let mut reward = 0.0f32;

    // Reward increasing build power.
    let prev_bp = prev_state.total_active_build_power(units) as f32;
    let next_bp = next_state.total_active_build_power(units) as f32;
    reward += ((next_bp - prev_bp) / 20.0).clamp(-10.0, 10.0);

    // Penalise mass stall: production halts when storage is empty.
    if next_state.economy.mass_storage < 1.0 {
        reward -= 5.0;
    }

    // Penalise mass overflow: income is wasted when storage is nearly full.
    let mass_cap = next_state.economy.mass_storage_cap;
    if mass_cap > 0.0 {
        let mass_ratio = (next_state.economy.mass_storage / mass_cap) as f32;
        if mass_ratio > 0.9 {
            reward -= 5.0 * (mass_ratio - 0.9) / 0.1;
        }
    }

    // Penalise energy stall severely: it throttles build power and mass income.
    if next_state.economy.energy_storage < 1.0 {
        reward -= 20.0;
    }

    reward
}
