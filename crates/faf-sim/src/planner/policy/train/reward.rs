//! Reward shaping for policy-gradient training.

use crate::sim::SimulationState;
use crate::units::{TechLevel, UnitKind, Units};

/// Compute a terminal bonus for reaching the goal.
///
/// A large positive reward encourages finishing, while a small time penalty
/// discourages very slow completions.
pub(crate) fn compute_terminal_bonus(state: &SimulationState, goal_reached: bool) -> f32 {
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
    prev_state: &SimulationState,
    next_state: &SimulationState,
    units: &Units,
) -> f32 {
    let mut reward = 0.0f32;

    // Reward increasing build power.
    let prev_bp = prev_state.total_active_build_power(units) as f32;
    let next_bp = next_state.total_active_build_power(units) as f32;
    reward += ((next_bp - prev_bp) / 20.0).clamp(-10.0, 10.0);

    // Reward increasing mass income. Mass and BP are coupled: mass income
    // must be turned into BP quickly, otherwise it overflows and is wasted.
    let prev_mass = prev_state.economy.net_mass_income as f32;
    let next_mass = next_state.economy.net_mass_income as f32;
    let mass_delta = (next_mass - prev_mass).clamp(-30.0, 30.0);
    reward += (mass_delta / 10.0).clamp(-10.0, 10.0);

    // Reward increasing power income. More BP consumes more energy, so the
    // agent must grow power to keep the expansion chain running.
    let prev_energy = prev_state.economy.net_energy_income as f32;
    let next_energy = next_state.economy.net_energy_income as f32;
    let energy_delta = (next_energy - prev_energy).clamp(-100.0, 100.0);
    reward += (energy_delta / 50.0).clamp(-5.0, 5.0);

    // Penalise high mass storage and overflow. A good player keeps mass low
    // by spending it on engineers and other projects quickly.
    let mass_cap = next_state.economy.mass_storage_cap;
    if mass_cap > 0.0 {
        let mass_ratio = (next_state.economy.mass_storage / mass_cap) as f32;
        if mass_ratio > 0.7 {
            reward -= 3.0 * (mass_ratio - 0.7) / 0.3;
        }
        if mass_ratio > 0.9 {
            reward -= 5.0 * (mass_ratio - 0.9) / 0.1;
        }
    }

    // Penalise energy stall severely: it throttles build power and mass income.
    if next_state.economy.energy_storage < 1.0 {
        reward -= 20.0;
    }

    // Small penalty for mass stall.
    if next_state.economy.mass_storage < 1.0 {
        reward -= 1.0;
    }

    reward
}

/// Tracks one-time tech milestones per episode and returns a bonus the first
/// time each milestone is reached.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct MilestoneTracker {
    t2_factory: bool,
    t3_factory: bool,
    t3_engineer: bool,
}

impl MilestoneTracker {
    /// Update milestone state after a successful action and return any newly
    /// earned bonuses.
    pub(crate) fn update(&mut self, state: &SimulationState, _units: &Units) -> f32 {
        let mut bonus = 0.0f32;

        if !self.t2_factory && state.has_completed_unit(&UnitKind::Factory(TechLevel::T2)) {
            self.t2_factory = true;
            bonus += 50.0;
        }
        if !self.t3_factory && state.has_completed_unit(&UnitKind::Factory(TechLevel::T3)) {
            self.t3_factory = true;
            bonus += 150.0;
        }
        if !self.t3_engineer && state.has_completed_unit(&UnitKind::Engineer(TechLevel::T3)) {
            self.t3_engineer = true;
            bonus += 300.0;
        }

        bonus
    }
}
