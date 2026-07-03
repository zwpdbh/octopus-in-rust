//! State featurization for the direction-only policy network.
//!
//! Converts a variable-size [`SimulationState`] into a fixed-size `Vec<f32>` that
//! the direction network consumes.

use crate::planner::core::PlannerConfig;
use crate::sim::SimulationState;
use crate::units::{TechLevel, UnitKind, Units};

/// Number of state features fed into the direction network.
///
/// This is a manual count of the values pushed by [`state_features`] below.
/// The vector is deliberately small and economy-centric: FAF build orders are
/// driven mainly by income, storage, build power, time, mex saturation, active
/// projects, and the few tech milestones that unlock the goal path.
///
/// Historical note: earlier experiments used a larger feature set (including a
/// 3-D "shortfall" vector). Those extra channels were removed because they did
/// not improve decisions and made the network wider for no benefit. The current
/// 11 features capture the same strategic information more compactly.
///
/// If you add or remove a feature, update this constant and the ordered list in
/// the doc comment of [`state_features`].
pub const STATE_FEATURE_COUNT: usize = 11;

/// Convert a simulator state into a fixed-length feature vector.
///
/// The 11 state features are intentionally economy-centric and small. Build
/// orders in FAF are driven mainly by income, build power, and tech tier, so
/// the network gets those directly instead of a huge one-hot unit roster.
///
/// The count of `features.push(...)` calls below must always equal
/// [`STATE_FEATURE_COUNT`]. The `debug_assert_eq!` at the end of this function
/// catches accidental drift.
///
/// Feature order:
/// 0. net mass income   (scaled by 100)
/// 1. net energy income (scaled by 1000)
/// 2. mass storage ratio
/// 3. energy storage ratio
/// 4. total active build power (scaled by 100)
/// 5. simulation time (scaled by 3600 s)
/// 6. active mex fraction of cap
/// 7. active project count (scaled by 10)
/// 8. has T2 factory
/// 9. has T3 factory
/// 10. has T3 engineer
pub fn state_features(state: &SimulationState, units: &Units, config: &PlannerConfig) -> Vec<f32> {
    let mut features = Vec::with_capacity(STATE_FEATURE_COUNT);
    let economy = &state.economy;

    // Income features: scaled so typical mid/late-game values land near [-1, 1]
    // before clamping. Energy is scaled by 1000 because it is usually an order
    // of magnitude larger than mass income.
    features.push(clamp((economy.net_mass_income / 100.0) as f32));
    features.push(clamp((economy.net_energy_income / 1000.0) as f32));

    // Storage ratios: near 0 means an impending stall; near 1 means income is
    // being wasted. The ratio is more useful than the absolute value because
    // storage capacity can vary.
    features.push(storage_ratio(
        economy.mass_storage,
        economy.mass_storage_cap,
    ));
    features.push(storage_ratio(
        economy.energy_storage,
        economy.energy_storage_cap,
    ));

    // Total build power determines how fast projects finish. Scaled by 100 so
    // typical values stay small after clamping.
    features.push(clamp(
        (state.total_active_build_power(units) / 100.0) as f32,
    ));

    // Game time gives the network a sense of phase. Scaled by one hour.
    features.push(clamp((state.time / 3600.0) as f32));

    // Eco-structure saturation: fraction of the configured mex cap. Near 1.0
    // means building more mexes is low-value or impossible.
    features.push(clamp(
        state.count_active_mex() as f32 / config.max_mex_count as f32,
    ));

    // Parallelism: how many builders are already committed. A high count means
    // fewer idle builders and fewer immediately executable options.
    let active_project_count = state
        .graph
        .graph
        .node_weights()
        .filter(|n| {
            matches!(
                n.state,
                crate::sim::UnitNodeState::Constructing { .. }
                    | crate::sim::UnitNodeState::Upgrading { .. }
            )
        })
        .count();
    features.push(clamp(active_project_count as f32 / 10.0));

    // Tech milestones: these gates unlock most of the rest of the goal path,
    // so the network receives them as explicit booleans instead of having to
    // infer them from the unit roster.
    //
    // - T2/T3 factories unlock the ability to build higher-tier engineers.
    // - T3 engineer is the concrete builder that can start the abstract goal
    //   (e.g. a T4 experimental). Without this flag the policy would have to
    //   deduce goal availability from the much larger unit roster.
    features.push(bool_f32(
        state.has_completed_unit(&UnitKind::Factory(TechLevel::T2)),
    ));
    features.push(bool_f32(
        state.has_completed_unit(&UnitKind::Factory(TechLevel::T3)),
    ));
    features.push(bool_f32(
        state.has_completed_unit(&UnitKind::Engineer(TechLevel::T3)),
    ));

    debug_assert_eq!(features.len(), STATE_FEATURE_COUNT);
    features
}

/// Clamp a value to a reasonable range and handle NaN.
fn clamp(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-10.0, 10.0)
    } else {
        0.0
    }
}

/// Convert a boolean to 0.0 or 1.0.
fn bool_f32(b: bool) -> f32 {
    if b {
        1.0
    } else {
        0.0
    }
}

/// Return the storage ratio, or 0.0 if capacity is zero.
fn storage_ratio(current: f64, cap: f64) -> f32 {
    if cap > 0.0 {
        clamp((current / cap) as f32)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::core::Goal;
    use crate::units::{TechLevel, UnitKind, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn state_feature_vector_has_expected_length() {
        let units = load_units();
        let goal = Goal {
            tech_level: TechLevel::T4,
            mass_cost: 28_000.0,
            energy_cost: 340_000.0,
            build_time: 46_250.0,
        };
        let _plan = units.plan_graph(goal);
        let state = SimulationState::new(&units, &[UnitKind::Commander]);
        let config = PlannerConfig::default();

        let features = state_features(&state, &units, &config);
        assert_eq!(features.len(), STATE_FEATURE_COUNT);
    }
}
