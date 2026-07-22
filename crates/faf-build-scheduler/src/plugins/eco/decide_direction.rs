//! Decide-direction lifecycle step.
//!
//! Computes per-direction confidence scores from the symbolic [`Observation`].

use bevy_ecs::prelude::*;

use faf_blueprints::TechLevel;

use super::observe::{
    EnergyMargin, MassIncomeVsTarget, MassMargin, MassProductionTier, Observation,
};

/// Confidence scores (0–100) for each economic direction.
///
/// Higher scores mean the observation suggests that direction is more urgent or
/// more appropriate right now.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirectionScores {
    /// Confidence that the next step should increase energy income.
    pub energy: u8,
    /// Confidence that the next step should increase mass income.
    pub mass_income: u8,
    /// Confidence that the next step should increase build power (engineers).
    pub build_power: u8,
    /// Confidence that the next step should advance to T2 tech.
    pub tech_t2: u8,
    /// Confidence that the next step should advance to T3 tech.
    pub tech_t3: u8,
}

/// Compute confidence scores from the current observation.
pub(crate) fn compute_direction_scores(obs: &Observation) -> DirectionScores {
    DirectionScores {
        energy: energy_score(obs),
        mass_income: mass_income_score(obs),
        build_power: build_power_score(obs),
        tech_t2: tech_score(obs, TechLevel::T2),
        tech_t3: tech_score(obs, TechLevel::T3),
    }
}

fn energy_score(obs: &Observation) -> u8 {
    match obs.energy_margin {
        EnergyMargin::Stalled => 100,
        EnergyMargin::Thin => 90,
        EnergyMargin::Unhealthy => 70,
        EnergyMargin::Healthy => 20,
        EnergyMargin::Surplus => 0,
    }
}

fn mass_income_score(obs: &Observation) -> u8 {
    match obs.mass_margin {
        MassMargin::Stall => 100,
        _ if obs.mass_income_vs_target == MassIncomeVsTarget::Below => 100,
        _ => 0,
    }
}

fn build_power_score(obs: &Observation) -> u8 {
    let idle = obs.idle_engineers.t1 + obs.idle_engineers.t2 + obs.idle_engineers.t3;
    if idle < 2 {
        30
    } else {
        0
    }
}

fn tech_score(obs: &Observation, desired: TechLevel) -> u8 {
    let tier_reached = match desired {
        TechLevel::T2 => obs.mass_production_tier == MassProductionTier::AtTech2,
        TechLevel::T3 => obs.mass_production_tier == MassProductionTier::AtTech3,
        _ => false,
    };
    if tier_reached && obs.mass_income_vs_target != MassIncomeVsTarget::Below {
        match desired {
            TechLevel::T3 => 100,
            TechLevel::T2 => 80,
            _ => 0,
        }
    } else {
        0
    }
}

/// Priority multipliers (1–10) for the three resource categories.
///
/// A higher value means actions that produce that resource are more favored.
/// The default is 5 (neutral). Scoring normalizes by dividing by 5, so the
/// default multiplier is 1.0.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PriorityTable {
    /// Priority for mass-income actions.
    pub mass: u8,
    /// Priority for energy-income actions.
    pub energy: u8,
    /// Priority for build-power (engineer) actions.
    pub build_power: u8,
}

/// Compute priority weights from the current observation.
///
/// * Mass is boosted when storage is low/`Stall` and reduced when it is high.
/// * Energy is boosted when the margin is thin/stalling and reduced when
///   strongly positive.
/// * Build power is boosted when mass is overflowing (so we spend mass on
///   engineers) and reduced when mass is scarce.
///
/// The priority is applied to the whole score, so the values are kept within a
/// range that cannot flip a higher-confidence direction behind a lower one.
pub(crate) fn compute_priority_table(obs: &Observation) -> PriorityTable {
    let mass = match obs.mass_margin {
        MassMargin::Stall => 10,
        MassMargin::Good => 7,
        MassMargin::Normal => 5,
        MassMargin::NeedToSpend => 3,
        MassMargin::Overflow => 1,
    };

    let energy = match obs.energy_margin {
        EnergyMargin::Stalled => 10,
        EnergyMargin::Thin => 9,
        EnergyMargin::Unhealthy => 7,
        EnergyMargin::Healthy => 4,
        EnergyMargin::Surplus => 1,
    };

    // Build-power priority is driven by mass storage: when mass is overflowing
    // we want to spend it on engineers; when mass is scarce we cannot afford
    // more build power.
    let build_power = match obs.mass_margin {
        MassMargin::Overflow => 10,
        MassMargin::NeedToSpend => 8,
        MassMargin::Normal => 5,
        MassMargin::Good => 3,
        MassMargin::Stall => 1,
    };

    PriorityTable {
        mass,
        energy,
        build_power,
    }
}

/// Compute per-direction confidence scores and priority weights from the
/// current observation and write them to the [`DirectionScores`] and
/// [`PriorityTable`] resources.
pub(crate) fn decide_eco_direction_system(
    observation: Res<Observation>,
    mut scores: ResMut<DirectionScores>,
    mut priorities: ResMut<PriorityTable>,
) {
    *scores = compute_direction_scores(&observation);
    *priorities = compute_priority_table(&observation);
}
