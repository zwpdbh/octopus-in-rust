//! Reward shaping for the standalone eco trainer.

use crate::engine::simulation::Simulation;
use crate::engine::EcoEngine;
use crate::planner::policy::train::config::TrainEcoConfig;
use crate::units::{TechLevel, UnitKind, Units};

/// Compute the reward for a single eco-training step.
///
/// The reward encourages growing mass income, building units faster, and
/// penalises resource stalls.
pub(crate) fn compute_eco_step_reward(
    prev_state: &Simulation,
    next_state: &Simulation,
    units: &Units,
    config: &TrainEcoConfig,
) -> f32 {
    let mass_income_delta = (next_state.engine.economy.net_mass_income.value()
        - prev_state.engine.economy.net_mass_income.value()) as f32;

    let mut reward = mass_income_delta * config.reward_mass_income_coef;

    // Reward reducing the simulated time needed to build a reference unit.
    reward += time_to_build_reward(prev_state, next_state, units, config);

    // Penalise stalls.
    if next_state.engine.economy.energy_storage.value() <= 1e-6
        && next_state.engine.economy.net_energy_income.value() < 0.0
    {
        reward -= config.energy_stall_penalty * config.dt as f32;
    }
    if next_state.engine.economy.mass_storage.value() <= 1e-6
        && next_state.engine.economy.net_mass_income.value() < 0.0
    {
        reward -= config.mass_stall_penalty * config.dt as f32;
    }

    reward
}

/// Reward for reducing the time needed to build a reference T1 mex.
///
/// The reference build power is the ACU's build rate. This isolates the
/// economy effect: a state with more income and storage can sustain a higher
/// effective build rate and finish the reference unit sooner.
fn time_to_build_reward(
    prev_state: &Simulation,
    next_state: &Simulation,
    units: &Units,
    config: &TrainEcoConfig,
) -> f32 {
    if config.reward_time_to_build_coef == 0.0 {
        return 0.0;
    }

    let Some(acu_def) = units.def(&UnitKind::Commander) else {
        return 0.0;
    };
    let build_power = acu_def.build_rate();

    let Some(cost) = units
        .build_cost(&UnitKind::Mex(TechLevel::T1))
        .map(|c| c.to_target_stats())
    else {
        return 0.0;
    };

    let ticks_per_second = (1.0 / config.dt).round() as u64;
    let prev_engine = EcoEngine::new(prev_state.engine.economy, ticks_per_second);
    let next_engine = EcoEngine::new(next_state.engine.economy, ticks_per_second);

    let cap_seconds = 300.0;
    let prev_time = prev_engine.time_to_finish(build_power, &cost, cap_seconds);
    let next_time = next_engine.time_to_finish(build_power, &cost, cap_seconds);

    match (prev_time, next_time) {
        (Ok(prev), Ok(next)) => (prev - next) as f32 * config.reward_time_to_build_coef,
        // Becoming able to finish the reference unit is a strong positive signal.
        (Err(_), Ok(_)) => 10.0 * config.reward_time_to_build_coef,
        // Becoming unable to finish is a strong negative signal.
        (Ok(_), Err(_)) => -10.0 * config.reward_time_to_build_coef,
        (Err(_), Err(_)) => 0.0,
    }
}

/// Bonus reward for reaching the target mass income.
pub(crate) fn eco_episode_bonus(final_state: &Simulation, target_mass_income: f64) -> f32 {
    if final_state.engine.economy.net_mass_income.value() >= target_mass_income {
        100.0
    } else {
        0.0
    }
}
