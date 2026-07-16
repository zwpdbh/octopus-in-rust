//! Effective build-factor calculation.

use crate::solver::single_task::state::{SolverState, EPS};

/// Compute the fraction of full build power that can be applied this tick.
///
/// The result is the minimum of:
/// - the energy-limited factor (available energy / energy drain),
/// - the mass-limited factor (available mass / mass drain),
/// - 1.0 (full power).
pub(crate) fn effective_factor(state: &SolverState, mass_drain: f64, energy_drain: f64) -> f64 {
    let energy_available = (state.energy_income - state.maintenance).max(0.0);

    let energy_factor = if energy_drain > EPS {
        ((state.energy + energy_available) / energy_drain).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let depleted = state.is_depleted();
    let eff = if depleted {
        (state.energy_income / (state.maintenance + energy_drain)).min(1.0)
    } else {
        1.0
    };
    let mass_income_scaled = state.mass_income * eff;

    let mass_factor = if mass_drain > EPS {
        ((state.mass + mass_income_scaled) / mass_drain).clamp(0.0, 1.0)
    } else {
        1.0
    };

    energy_factor.min(mass_factor).min(1.0)
}
