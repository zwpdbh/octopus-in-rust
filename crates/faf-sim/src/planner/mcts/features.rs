//! State featurization for the hierarchical policy networks.
//!
//! Converts a variable-size [`GraphState`] into a fixed-size `Vec<f32>` that
//! the macro network, build-power network, and engineer-squad network consume.
//! The base feature vector is shared; the macro network additionally receives
//! the previous-tick engineer shortfall, the power network receives a one-hot
//! encoding of the selected edge, and the squad network receives the target
//! build power.

use crate::planner::core::PlannerConfig;
use crate::sim::GraphState;
use crate::units::{TechLevel, UnitKind, Units};

/// Number of base state features.
pub const STATE_FEATURE_COUNT: usize = 13;

/// Number of engineer shortfall feedback features appended to the macro network's
/// input ([T1, T2, T3]).
pub const SHORTFALL_FEATURE_COUNT: usize = 3;

/// Convert a simulator state into a fixed-length feature vector.
///
/// The 13 state features are intentionally economy-centric and small. Build
/// orders in FAF are driven mainly by income, build power, and tech tier, so
/// the network gets those directly instead of a huge one-hot unit roster.
///
/// Feature order:
/// 0. net mass income   (scaled by 100)
/// 1. net energy income (scaled by 1000)
/// 2. mass storage ratio
/// 3. energy storage ratio
/// 4. total active build power (scaled by 100)
/// 5. simulation time (scaled by 3600 s)
/// 6. active mex fraction of cap
/// 7. active pgen fraction of cap
/// 8. active energy storage fraction of cap
/// 9. active project count (scaled by 10)
/// 10. has T2 factory
/// 11. has T3 factory
/// 12. has T3 engineer
pub fn state_features(state: &GraphState, units: &Units, config: &PlannerConfig) -> Vec<f32> {
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

    // Eco-structure saturation: fractions of the configured caps. Near 1.0
    // means building more mexes/pgens is low-value or impossible.
    features.push(clamp(
        state.count_active_mex() as f32 / config.max_mex_count as f32,
    ));
    features.push(clamp(
        state.count_active_pgen() as f32 / config.max_pgen_count as f32,
    ));
    features.push(clamp(
        state.count_active_energy_storage() as f32 / config.max_energy_storage_count as f32,
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

/// Append the previous-tick engineer shortfall to the base state features.
///
/// The macro network receives both economy/state features and explicit feedback
/// that the previous action wanted more engineers of a given tech than were
/// available. This helps it learn to build/upgrade engineers before retrying
/// an edge that previously starved.
pub fn state_features_with_shortfall(
    state: &GraphState,
    units: &Units,
    config: &PlannerConfig,
    shortfall: [f32; SHORTFALL_FEATURE_COUNT],
) -> Vec<f32> {
    let mut features = state_features(state, units, config);
    features.extend_from_slice(&shortfall);
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
    use crate::units::{UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn state_feature_vector_has_expected_length() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let _plan = units.plan_graph(&goal).unwrap();
        let state = GraphState::new(&units, &[UnitKind::Commander]);
        let config = PlannerConfig::default();

        let features = state_features(&state, &units, &config);
        assert_eq!(features.len(), STATE_FEATURE_COUNT);
    }
}
